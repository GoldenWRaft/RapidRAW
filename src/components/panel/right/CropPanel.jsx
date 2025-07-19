import { RotateCcw, X, RectangleHorizontal, RectangleVertical, FlipHorizontal, FlipVertical, RotateCw } from 'lucide-react';
import { useState, useEffect, useCallback, useMemo } from 'react';
import { INITIAL_ADJUSTMENTS } from '../../../utils/adjustments';
import clsx from 'clsx';

const PRESETS = [
  { name: 'Free', value: null },
  { name: 'Original', value: 'original' },
  { name: '1:1', value: 1 },
  { name: '5:4', value: 5 / 4 },
  { name: '4:3', value: 4 / 3 },
  { name: '3:2', value: 3 / 2 },
  { name: '16:9', value: 16 / 9 },
];

const doesRatioMatchPreset = (ratio, preset, originalImage) => {
  if (preset.value === null && ratio === null) return true;

  let presetBaseRatio = preset.value;
  if (preset.value === 'original') {
    presetBaseRatio = originalImage?.width && originalImage?.height
      ? originalImage.width / originalImage.height
      : null;
  }

  if (!presetBaseRatio || !ratio) return presetBaseRatio === ratio;

  return Math.abs(ratio - presetBaseRatio) < 0.001 || Math.abs(ratio - (1 / presetBaseRatio)) < 0.001;
};

const ToolButton = ({ icon: Icon, label, onClick, isActive = false }) => (
  <button
    onClick={onClick}
    className={clsx(
      'flex flex-col items-center justify-center p-3 rounded-lg transition-colors text-text-secondary',
      'hover:bg-card-active hover:text-text-primary',
      isActive ? 'bg-surface text-text-primary' : 'bg-surface'
    )}
  >
    <Icon size={20} />
    <span className="text-xs mt-1.5">{label}</span>
  </button>
);

export default function CropPanel({ selectedImage, adjustments, setAdjustments }) {
  const [customW, setCustomW] = useState('');
  const [customH, setCustomH] = useState('');

  const { aspectRatio, rotation = 0, flipHorizontal = false, flipVertical = false } = adjustments;
  const activePreset = PRESETS.find(p => doesRatioMatchPreset(aspectRatio, p, selectedImage));

  let orientation = 'horizontal';
  if (activePreset && activePreset.value && activePreset.value !== 1) {
    let baseRatio = activePreset.value;
    if (activePreset.value === 'original') {
      baseRatio = selectedImage?.width && selectedImage?.height ? selectedImage.width / selectedImage.height : null;
    }
    if (baseRatio && Math.abs(aspectRatio - baseRatio) > 0.001) {
      orientation = 'vertical';
    }
  }

  const isCustomActive = aspectRatio !== null && !activePreset;

  useEffect(() => {
    if (isCustomActive && aspectRatio) {
      const w = Math.abs(aspectRatio * 100).toFixed(1);
      const h = (100).toFixed(1);
      setCustomW(w);
      setCustomH(h);
    } else {
      setCustomW('');
      setCustomH('');
    }
  }, [isCustomActive, aspectRatio]);

  const handleCustomInputChange = (w, h) => {
    setCustomW(w);
    setCustomH(h);
    const numW = parseFloat(w);
    const numH = parseFloat(h);
    if (numW > 0 && numH > 0) {
      const newAspectRatio = numW / numH;
      if (aspectRatio !== newAspectRatio) {
        setAdjustments(prev => ({ ...prev, aspectRatio: newAspectRatio, crop: null }));
      }
    }
  };

  const handlePresetClick = (preset) => {
    let baseRatio = preset.value;
    if (preset.value === 'original') {
      baseRatio = selectedImage?.width && selectedImage?.height
        ? selectedImage.width / selectedImage.height
        : null;
    }

    let newAspectRatio = baseRatio;
    if (baseRatio && baseRatio !== 1 && orientation === 'vertical') {
      newAspectRatio = 1 / baseRatio;
    }

    setAdjustments(prev => ({ ...prev, aspectRatio: newAspectRatio, crop: null }));
  };

  const handleOrientationToggle = useCallback(() => {
    if (aspectRatio && aspectRatio !== 1) {
      setAdjustments(prev => ({
        ...prev,
        aspectRatio: 1 / prev.aspectRatio,
        crop: null
      }));
    }
  }, [aspectRatio, setAdjustments]);

  const handleReset = () => {
    const originalAspectRatio = selectedImage?.width && selectedImage?.height
      ? selectedImage.width / selectedImage.height
      : null;

    setAdjustments(prev => ({
      ...prev,
      crop: INITIAL_ADJUSTMENTS.crop,
      aspectRatio: originalAspectRatio,
      rotation: INITIAL_ADJUSTMENTS.rotation || 0,
      flipHorizontal: INITIAL_ADJUSTMENTS.flipHorizontal || false,
      flipVertical: INITIAL_ADJUSTMENTS.flipVertical || false,
    }));
  };

  const isPresetActive = (preset) => preset === activePreset;
  const isOrientationToggleDisabled = !aspectRatio || aspectRatio === 1;

  const fineRotation = useMemo(() => {
    const total = rotation || 0;
    const remainder = total % 90;
    if (remainder > 45) return remainder - 90;
    if (remainder < -45) return remainder + 90;
    return remainder;
  }, [rotation]);

  const handleFineRotationChange = (e) => {
    const newFineRotation = parseFloat(e.target.value);
    const baseRotation = rotation - fineRotation;
    setAdjustments(prev => ({ ...prev, rotation: baseRotation + newFineRotation }));
  };

  const handleStepRotate = (degrees) => {
    setAdjustments(prev => ({ ...prev, rotation: (prev.rotation || 0) + degrees }));
  };

  const resetFineRotation = () => {
    const baseRotation = rotation - fineRotation;
    setAdjustments(prev => ({ ...prev, rotation: baseRotation }));
  };

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 flex justify-between items-center flex-shrink-0 border-b border-surface">
        <h2 className="text-xl font-bold text-primary text-shadow-shiny">Crop & Transform</h2>
        <button
          onClick={handleReset}
          className="p-2 rounded-full hover:bg-surface transition-colors"
          title="Reset All"
        >
          <RotateCcw size={18} />
        </button>
      </div>

      <div className="flex-grow overflow-y-auto p-4 text-text-secondary space-y-6">
        {selectedImage ? (
          <>
            <div className="space-y-4">
              <div className="flex justify-between items-center">
                <p className="text-sm mb-3 font-semibold text-text-primary">Aspect Ratio</p>
                <button
                  onClick={handleOrientationToggle}
                  className="p-1.5 rounded-md hover:bg-surface disabled:text-text-tertiary disabled:cursor-not-allowed"
                  title="Switch Orientation"
                  disabled={isOrientationToggleDisabled}
                >
                  {orientation === 'vertical' ? <RectangleVertical size={16} /> : <RectangleHorizontal size={16} />}
                </button>
              </div>
              <div className="grid grid-cols-3 gap-2">
                {PRESETS.map(preset => (
                  <button
                    key={preset.name}
                    onClick={() => handlePresetClick(preset)}
                    className={clsx('px-2 py-1.5 text-sm rounded-md transition-colors',
                      isPresetActive(preset) ? 'bg-surface text-text-primary' : 'bg-surface hover:bg-card-active'
                    )}
                  >
                    {preset.name}
                  </button>
                ))}
              </div>
              <div className="mt-3">
                <button
                  onClick={() => {
                    const baseRatio = 1.618;
                    const newAspectRatio = orientation === 'vertical' ? 1 / baseRatio : baseRatio;
                    setAdjustments(prev => ({ ...prev, aspectRatio: newAspectRatio, crop: null }))
                  }}
                  className={clsx('w-full px-2 py-1.5 text-sm rounded-md transition-colors',
                    isCustomActive ? 'bg-accent text-white' : 'bg-surface hover:bg-card-active'
                  )}
                >
                  Custom
                </button>
                <div className={clsx('mt-2 bg-surface p-2 rounded-md transition-opacity', isCustomActive ? 'opacity-100' : 'opacity-50 pointer-events-none')}>
                  <div className="flex items-center justify-center gap-2">
                    <input type="number" placeholder="W" value={customW} onChange={(e) => handleCustomInputChange(e.target.value, customH)} className="w-full bg-bg-primary text-center rounded-md p-1 border border-surface focus:border-accent focus:ring-accent" min="1" />
                    <X size={16} className="text-text-tertiary flex-shrink-0" />
                    <input type="number" placeholder="H" value={customH} onChange={(e) => handleCustomInputChange(customW, e.target.value)} className="w-full bg-bg-primary text-center rounded-md p-1 border border-surface focus:border-accent focus:ring-accent" min="1" />
                  </div>
                </div>
              </div>
            </div>

            <div className="space-y-3">
              <p className="text-sm mb-3 font-semibold text-text-primary">Rotation</p>
              <div className="flex justify-between items-center">
                <span className="font-mono text-lg text-text-primary">{rotation.toFixed(1)}°</span>
                <button onClick={resetFineRotation} title="Reset Fine Rotation" className="p-1.5 rounded-full hover:bg-surface">
                  <RotateCcw size={14} />
                </button>
              </div>
              <input
                type="range"
                min="-45"
                max="45"
                step="0.1"
                value={fineRotation}
                onChange={handleFineRotationChange}
                className="w-full h-2 bg-surface rounded-lg appearance-none cursor-pointer accent-accent"
              />
            </div>

            <div className="space-y-4">
              <p className="text-sm mb-3 font-semibold text-text-primary">Tools</p>
              <div className="grid grid-cols-2 gap-2">
                <ToolButton icon={RotateCcw} label="Rotate Left" onClick={() => handleStepRotate(-90)} />
                <ToolButton icon={RotateCw} label="Rotate Right" onClick={() => handleStepRotate(90)} />
                <ToolButton icon={FlipHorizontal} label="Flip Horiz" onClick={() => setAdjustments(prev => ({ ...prev, flipHorizontal: !prev.flipHorizontal }))} isActive={flipHorizontal} />
                <ToolButton icon={FlipVertical} label="Flip Vert" onClick={() => setAdjustments(prev => ({ ...prev, flipVertical: !prev.flipVertical }))} isActive={flipVertical} />
              </div>
            </div>
          </>
        ) : (
          <p className="text-center text-text-tertiary mt-4">No image selected.</p>
        )}
      </div>
    </div>
  );
}