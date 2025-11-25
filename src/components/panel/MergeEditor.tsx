import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { 
  Check, X, ZoomIn, ZoomOut, Loader2, 
  Layers, Aperture, Eye, EyeOff, Maximize 
} from 'lucide-react';
import clsx from 'clsx';

// -- Types --
export interface AlignedBracketFrame {
  index: number;
  path: string;
  transform: number[][];
  preview_base64: string;
}

interface Props {
  frames: AlignedBracketFrame[];
  onClose: () => void;
  onSave: (settings: MergeSettings) => void;
}

export interface MergeSettings {
  mode: 'exposure' | 'focus';
  enabledIndices: boolean[];
  param: number;
}

export default function MergeEditor({ frames, onClose, onSave }: Props) {
  // -- State --
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [mergedImageSrc, setMergedImageSrc] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [processingTime, setProcessingTime] = useState(0);
  
  // Merge Parameters
  const [mode, setMode] = useState<'exposure' | 'focus'>('exposure');
  const [layerVisibility, setLayerVisibility] = useState<boolean[]>(
    new Array(frames.length).fill(true)
  );
  const [soloLayerIndex, setSoloLayerIndex] = useState<number | null>(null);

  // Viewport State
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [isDragging, setIsDragging] = useState(false);
  const lastMousePos = useRef({ x: 0, y: 0 });
  const imageObj = useRef<HTMLImageElement | null>(null);
  // Add state
  const [isComparing, setIsComparing] = useState(false);
  const [param, setParam] = useState(0.5);

  // Helper to get comparison image
  const getCompareImage = () => {
      // If a layer is selected (but not soloed), compare against that.
      // Otherwise compare against the "Anchor" (middle image).
      // We use the raw preview_base64 for comparison.
      const targetIndex = soloLayerIndex !== null ? soloLayerIndex : Math.floor(frames.length / 2);
      return frames[targetIndex].preview_base64;
  }

  // -- 1. Pipeline Trigger --
  useEffect(() => {
    // If Solo mode is active, show that frame's RAW preview immediately (no backend call needed)
    if (soloLayerIndex !== null) {
        setMergedImageSrc(frames[soloLayerIndex].preview_base64);
        return;
    }

    let active = true;
    const fetchMerge = async () => {
      setLoading(true);
      const start = performance.now();
      try {
        const base64: string = await invoke('merge_bracket_images', { 
          frames,
          mode,
          enabled: layerVisibility,
          param
        });

        if (active) {
            setMergedImageSrc(base64);
            setProcessingTime(Math.round(performance.now() - start));
        }
      } catch (err) {
        console.error("Merge failed:", err);
      } finally {
        if (active) setLoading(false);
      }
    };

    // Debounce slightly to prevent rapid clicking from spawning too many GPU tasks
    const timer = setTimeout(fetchMerge, 50);
    return () => { 
        active = false; 
        clearTimeout(timer);
    };
  }, [frames, mode, layerVisibility, soloLayerIndex, param]);

  // -- 2. Canvas Rendering --
  useEffect(() => {
      if(!mergedImageSrc) return;
      const img = new Image();
      img.onload = () => {
          imageObj.current = img;
          if (zoom === 1 && offset.x === 0) fitImageToCanvas(img); // Auto-fit on first load
          renderCanvas(); 
      };
      img.src = mergedImageSrc;
  }, [mergedImageSrc]);

  const renderCanvas = () => {
      const canvas = canvasRef.current;
      if (!canvas || !imageObj.current) return;
      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      // Handle High-DPI displays
      const dpr = window.devicePixelRatio || 1;
      const rect = canvas.getBoundingClientRect();
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
      ctx.scale(dpr, dpr);

      // Draw Background (Checkered pattern for transparency if needed, or dark)
      ctx.fillStyle = '#111';
      ctx.fillRect(0, 0, rect.width, rect.height);

      // Draw Image
      ctx.save();
      // Center pivot
      ctx.translate(rect.width/2 + offset.x, rect.height/2 + offset.y);
      ctx.scale(zoom, zoom);
      // Draw centered
      if (isComparing) {
        // Load comparison image dynamically or pre-load it
        const compareSrc = getCompareImage();
        const compImg = new Image();
        compImg.src = compareSrc;
        // (Note: In real implementation, pre-load these images into refs to avoid flashing)
        ctx.drawImage(compImg, -imageObj.current.width/2, -imageObj.current.height/2, imageObj.current.width, imageObj.current.height);
        
        // Draw "Original" label
        ctx.fillStyle = "white";
        ctx.font = "bold 20px sans-serif";
        ctx.fillText("ORIGINAL (Frame " + (Math.floor(frames.length/2)+1) + ")", -imageObj.current.width/2 + 20, -imageObj.current.height/2 + 40);
      } else 
        ctx.drawImage(
          imageObj.current, 
          -imageObj.current.width/2, 
          -imageObj.current.height/2
        );
      
      // Solo Indicator Border
      if (soloLayerIndex !== null) {
          ctx.strokeStyle = '#3b82f6'; // Blue border
          ctx.lineWidth = 4 / zoom;
          ctx.strokeRect(
            -imageObj.current.width/2, 
            -imageObj.current.height/2, 
            imageObj.current.width, 
            imageObj.current.height
          );
      }
      ctx.restore();
  };

  // Re-render on interaction
  useEffect(renderCanvas, [isComparing, zoom, offset, soloLayerIndex]);

  // -- 3. Helpers --
  const fitImageToCanvas = (img: HTMLImageElement) => {
    if (!canvasRef.current) return;
    const rect = canvasRef.current.getBoundingClientRect();
    const scale = Math.min(rect.width / img.width, rect.height / img.height) * 0.9;
    setZoom(scale);
    setOffset({ x: 0, y: 0 });
  };

  const toggleLayer = (index: number) => {
      setLayerVisibility(prev => {
          const next = [...prev];
          next[index] = !next[index];
          return next;
      });
  };

  const toggleSolo = (index: number) => {
      setSoloLayerIndex(prev => prev === index ? null : index);
  };

  // -- 4. Input Handlers --
  const handleWheel = (e: React.WheelEvent) => {
    const zoomSensitivity = 0.001;
    const newZoom = Math.max(0.05, Math.min(10, zoom - e.deltaY * zoomSensitivity));
    setZoom(newZoom);
  };
  const handleMouseDown = (e: React.MouseEvent) => {
    setIsDragging(true);
    lastMousePos.current = { x: e.clientX, y: e.clientY };
  };
  const handleMouseMove = (e: React.MouseEvent) => {
    if (!isDragging) return;
    const dx = e.clientX - lastMousePos.current.x;
    const dy = e.clientY - lastMousePos.current.y;
    lastMousePos.current = { x: e.clientX, y: e.clientY };
    setOffset(prev => ({ x: prev.x + dx, y: prev.y + dy }));
    renderCanvas();
  };
  const handleMouseUp = () => setIsDragging(false);

  return (
    <div className="fixed inset-0 z-[100] bg-[#1e1e1e] flex font-sans select-none text-gray-200 top-10">
      
      {/* --- LEFT SIDEBAR: LAYERS --- */}
      <div className="w-72 bg-[#252525] border-r border-gray-800 flex flex-col shadow-2xl">
          <div className="h-16 flex items-center px-4 border-b border-gray-700 bg-[#2a2a2a]">
              <Layers size={18} className="mr-2 text-blue-400"/>
              <span className="font-bold text-sm tracking-wide">SOURCE FRAMES</span>
              <span className="ml-auto text-xs text-gray-500 bg-black/20 px-2 py-1 rounded">{frames.length} frames</span>
          </div>
          
          <div className="flex-1 overflow-y-auto p-2 space-y-1 custom-scrollbar">
              {frames.map((frame, i) => (
                  <div 
                    key={frame.path} 
                    className={clsx(
                        "group relative flex items-center p-2 rounded border transition-all cursor-pointer select-none",
                        soloLayerIndex === i 
                            ? "border-blue-500 bg-blue-500/10" 
                            : "border-transparent hover:bg-[#333] hover:border-gray-600",
                        !layerVisibility[i] && "opacity-40 grayscale"
                    )}
                    onClick={() => toggleSolo(i)}
                  >
                      {/* Thumbnail */}
                      <div className="w-12 h-9 bg-black/50 rounded overflow-hidden flex-shrink-0 relative">
                         <img src={frame.preview_base64} className="w-full h-full object-cover" />
                         <div className="absolute bottom-0 right-0 bg-black/60 text-[8px] px-1 text-white">{i+1}</div>
                      </div>
                      
                      {/* Info */}
                      <div className="ml-3 flex-1 min-w-0">
                          <p className="text-xs font-medium text-gray-300 truncate">{frame.path.split(/[\\/]/).pop()}</p>
                          <p className="text-[10px] text-gray-500 mt-0.5">
                             {soloLayerIndex === i ? <span className="text-blue-400 font-bold">SOLO VIEW</span> : `Frame ${i+1}`}
                          </p>
                      </div>

                      {/* Toggle */}
                      <button 
                        onClick={(e) => { e.stopPropagation(); toggleLayer(i); }}
                        className={clsx(
                            "p-1.5 rounded hover:bg-white/10 transition-colors",
                            layerVisibility[i] ? "text-gray-400 hover:text-white" : "text-red-400 hover:text-red-300"
                        )}
                        title={layerVisibility[i] ? "Disable Frame" : "Enable Frame"}
                      >
                          {layerVisibility[i] ? <Eye size={14} /> : <EyeOff size={14} />}
                      </button>
                  </div>
              ))}
          </div>

          {/* Parameter Slider */}
          <div className="mt-4 px-1">
              <div className="flex justify-between text-[10px] text-gray-400 mb-1">
                  <span>{mode === 'exposure' ? 'Bias (Dark ↔ Light)' : 'Hardness (Soft ↔ Sharp)'}</span>
                  <span>{Math.round(param * 100)}%</span>
              </div>
              <input 
                  type="range" min="0" max="1" step="0.05"
                  value={param}
                  onChange={(e) => setParam(parseFloat(e.target.value))}
                  className="w-full h-1 bg-gray-700 rounded-lg appearance-none cursor-pointer accent-blue-500"
              />
          </div>
          
          {/* Algorithm Control */}
          <div className="p-4 border-t border-gray-700 bg-[#2a2a2a]">
            <div className="flex items-center justify-between mb-3">
                <p className="text-xs font-bold text-gray-400 uppercase">Algorithm</p>
                {loading && <Loader2 size={12} className="animate-spin text-blue-400"/>}
            </div>
            
            <div className="grid grid-cols-2 gap-2">
                <button 
                    onClick={() => setMode('exposure')} 
                    className={clsx(
                        "flex flex-col items-center justify-center p-3 rounded border transition-all",
                        mode==='exposure' 
                            ? "bg-blue-600 border-blue-500 text-white shadow-lg" 
                            : "bg-[#333] border-gray-700 text-gray-400 hover:bg-[#3d3d3d]"
                    )}
                >
                    <Layers size={20} className="mb-1"/>
                    <span className="text-[10px] font-bold uppercase">Exposure</span>
                </button>
                <button 
                    onClick={() => setMode('focus')} 
                    className={clsx(
                        "flex flex-col items-center justify-center p-3 rounded border transition-all",
                        mode==='focus' 
                            ? "bg-blue-600 border-blue-500 text-white shadow-lg" 
                            : "bg-[#333] border-gray-700 text-gray-400 hover:bg-[#3d3d3d]"
                    )}
                >
                    <Aperture size={20} className="mb-1"/>
                    <span className="text-[10px] font-bold uppercase">Focus Stack</span>
                </button>
            </div>
          </div>
      </div>

      {/* --- MAIN CANVAS AREA --- */}
      <div className="flex-1 flex flex-col relative bg-[#050505]">
        {/* Top Toolbar */}
        <div className="h-14 flex items-center justify-between px-6 border-b border-gray-800 bg-[#1a1a1a]">
             <div className="flex items-center gap-3">
                <h2 className="font-bold text-gray-200">Merge Preview</h2>
                <div className="h-4 w-px bg-gray-700 mx-2"/>
                <span className="text-xs text-gray-500 font-mono">
                    {loading ? "Processing..." : `Time: ${processingTime}ms`}
                </span>
             </div>
             
             <div className="flex items-center gap-3">
                 <button 
                    onMouseDown={() => setIsComparing(true)}
                    onMouseUp={() => setIsComparing(false)}
                    onMouseLeave={() => setIsComparing(false)}
                    className="px-4 py-2 text-xs font-medium text-gray-400 hover:text-white hover:bg-white/5 rounded transition-colors"
                >
                    Compare
                </button>
                 <button 
                    onClick={onClose} 
                    className="px-4 py-2 text-xs font-medium text-gray-400 hover:text-white hover:bg-white/5 rounded transition-colors"
                 >
                    Cancel
                 </button>
                 <button 
                    onClick={() => onSave({ mode, enabledIndices: layerVisibility, param })} 
                    disabled={loading}
                    className="flex items-center gap-2 px-5 py-2 bg-green-600 hover:bg-green-500 text-white text-xs font-bold rounded shadow-lg hover:shadow-green-500/20 transition-all disabled:opacity-50"
                 >
                    <Check size={14} strokeWidth={3} />
                    EXPORT RESULT
                 </button>
             </div>
        </div>

        {/* Viewport */}
        <div className="flex-1 relative overflow-hidden">
            <canvas 
                ref={canvasRef}
                className="w-full h-full cursor-move block"
                onWheel={handleWheel}
                onMouseDown={handleMouseDown}
                onMouseMove={handleMouseMove}
                onMouseUp={handleMouseUp}
                onMouseLeave={handleMouseUp}
            />
            
            {/* Controls Overlay */}
            <div className="absolute bottom-6 left-1/2 -translate-x-1/2 flex gap-2 bg-[#1a1a1a]/90 backdrop-blur border border-gray-700 rounded-full p-1.5 shadow-2xl">
                <button 
                    onClick={() => setZoom(z => z*0.8)} 
                    className="p-2 hover:bg-white/10 rounded-full text-gray-300"
                    title="Zoom Out"
                >
                    <ZoomOut size={18}/>
                </button>
                <span className="flex items-center justify-center w-12 text-xs font-mono text-gray-400 select-none">
                    {Math.round(zoom * 100)}%
                </span>
                <button 
                    onClick={() => setZoom(z => z*1.2)} 
                    className="p-2 hover:bg-white/10 rounded-full text-gray-300"
                    title="Zoom In"
                >
                    <ZoomIn size={18}/>
                </button>
                <div className="w-px h-4 bg-gray-700 self-center mx-1"/>
                <button 
                    onClick={() => imageObj.current && fitImageToCanvas(imageObj.current)}
                    className="p-2 hover:bg-white/10 rounded-full text-gray-300"
                    title="Fit to Screen"
                >
                    <Maximize size={18}/>
                </button>
            </div>
        </div>
      </div>
    </div>
  );
}