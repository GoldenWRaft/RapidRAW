// import { Dialog, DialogContent, DialogOverlay } from '../ui/dialog'; // Adapting to your UI library if exists, or standard HTML
import { X, Check } from 'lucide-react';
import { useState } from 'react';

interface AlignedBracketFrame {
  index: number;
  path: string;
  transform: number[][];
  preview_base64: string;
}

interface Props {
  isOpen: boolean;
  onClose: () => void;
  frames: AlignedBracketFrame[];
  onConfirm: () => void;
}

export default function BracketReviewModal({ isOpen, onClose, frames, onConfirm }: Props) {
  const [opacity, setOpacity] = useState(0.5);
  const [blendMode, setBlendMode] = useState<any>('normal');

  if (!isOpen) return null;

  // Sort by index so Anchor is usually in middle or specific order
  const sortedFrames = [...frames].sort((a, b) => a.index - b.index);

  return (
    <div className="fixed inset-0 z-[999] flex items-center justify-center bg-black/80 backdrop-blur-sm">
      <div className="bg-[#1e1e1e] w-[90vw] h-[90vh] flex flex-col rounded-lg overflow-hidden border border-gray-700">
        
        {/* Header */}
        <div className="h-12 border-b border-gray-700 flex items-center justify-between px-4 bg-[#252525]">
          <span className="font-medium text-gray-200">Review Alignment</span>
          <div className="flex gap-2">
             <button onClick={onClose} className="p-2 hover:bg-red-500/20 rounded"><X size={18} /></button>
          </div>
        </div>

        {/* Viewport */}
        <div className="flex-1 relative overflow-hidden bg-black flex items-center justify-center">
          <div className="relative w-full h-full">
            {sortedFrames.map((frame, i) => (
              <img
                key={frame.path}
                src={frame.preview_base64} // Tauri asset protocol
                className="absolute top-0 left-0 w-full h-full object-contain origin-top-left transition-opacity duration-200"
                style={{
                  opacity: i === 0 ? 1 : opacity, // Keep first image solid, others ghosted
                  mixBlendMode: i === 0 ? 'normal' : blendMode,
                  // Apply the Matrix. Note: CSS Matrix3d is Column-Major and flat
                  transform: convertToCssMatrix(frame.transform)
                }}
              />
            ))}
          </div>
        </div>

        {/* Controls */}
        <div className="h-16 border-t border-gray-700 bg-[#252525] flex items-center gap-6 px-6">
          <div className="flex flex-col w-48">
            <label className="text-xs text-gray-400 mb-1">Overlay Opacity</label>
            <input 
              type="range" 
              min="0" max="1" step="0.01" 
              value={opacity} 
              onChange={(e) => setOpacity(parseFloat(e.target.value))}
              className="accent-blue-500" 
            />
          </div>
          
          <div className="flex gap-2">
            <button 
                onClick={() => setBlendMode('normal')}
                className={`px-3 py-1 rounded text-sm ${blendMode === 'normal' ? 'bg-blue-600' : 'bg-gray-700'}`}
            >
                Normal
            </button>
            <button 
                onClick={() => setBlendMode('difference')}
                className={`px-3 py-1 rounded text-sm ${blendMode === 'difference' ? 'bg-blue-600' : 'bg-gray-700'}`}
            >
                Difference (Check Alignment)
            </button>
          </div>

          <div className="flex-1" />
          
          <button 
            onClick={onConfirm} 
            className="flex items-center gap-2 px-6 py-2 bg-green-600 hover:bg-green-500 text-white rounded font-medium"
          >
            <Check size={18} />
            Merge Images
          </button>
        </div>
      </div>
    </div>
  );
}

// Helper to convert Rust 4x4 (Row-major in JSON?) to CSS Matrix3d (Column-major flattened)
function convertToCssMatrix(mat: number[][]): string {
  // Check if Identity
  const isIdentity = mat[0][0] === 1 && mat[0][1] === 0; // rough check
  if (isIdentity) return 'none';

  // CSS matrix3d(a1, b1, c1, d1, a2, b2, c2, d2, a3, b3, c3, d3, a4, b4, c4, d4)
  // Our input `mat` is likely [[row1], [row2], [row3], [row4]] based on previous logs.
  // We need to transpose it for CSS (Column Major).
  
  const m = mat;
  const flat = [
    m[0][0], m[1][0], m[2][0], m[3][0], // Column 1
    m[0][1], m[1][1], m[2][1], m[3][1], // Column 2
    m[0][2], m[1][2], m[2][2], m[3][2], // Column 3
    m[0][3], m[1][3], m[2][3], m[3][3]  // Column 4
  ];

  return `matrix3d(${flat.join(',')})`;
}