import { memo } from 'react';
import { Eye, EyeOff, ArrowLeft, Maximize, Loader2, Undo, Redo, Waves } from 'lucide-react';
import clsx from 'clsx';

const EditorToolbar = memo(({ 
  onBackToLibrary, selectedImage, isLoading, onToggleShowOriginal, showOriginal, 
  onToggleFullScreen, isFullScreenLoading,
  onUndo, onRedo, canUndo, canRedo,
  isWaveformVisible, onToggleWaveform 
}) => (
  <div className="relative flex-shrink-0 flex justify-between items-center px-4 h-14">
    <button onClick={onBackToLibrary} className="bg-surface text-text-primary p-2 rounded-full hover:bg-card-active transition-colors" title="Back to Library">
      <ArrowLeft size={20} />
    </button>

    <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 bg-surface text-text-secondary text-xs px-4 py-2 rounded-full select-none truncate flex items-center gap-2 max-w-[50%]">
      <span className="font-medium text-text-primary truncate">{selectedImage.path.split(/[\/\\]/).pop()}</span>
      {isLoading && <Loader2 size={12} className="animate-spin" />}
      {selectedImage.width > 0 && ` - ${selectedImage.width} × ${selectedImage.height}`}
    </div>

    <div className="flex items-center gap-2">
      <button onClick={onUndo} disabled={!canUndo} className="bg-surface text-text-primary p-2 rounded-full hover:bg-card-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed" title="Undo (Ctrl+Z)">
        <Undo size={20} />
      </button>
      <button onClick={onRedo} disabled={!canRedo} className="bg-surface text-text-primary p-2 rounded-full hover:bg-card-active transition-colors disabled:opacity-50 disabled:cursor-not-allowed" title="Redo (Ctrl+Y)">
        <Redo size={20} />
      </button>
      
      <button 
        onClick={onToggleWaveform} 
        className={clsx(
          "p-2 rounded-full transition-colors",
          isWaveformVisible 
            ? "bg-accent text-button-text hover:bg-accent/90 hover:text-button-text" 
            : "bg-surface hover:bg-card-active text-text-primary"
        )} 
        title="Toggle Waveform (W)"
      >
        <Waves size={20} />
      </button>

      <button 
        onClick={onToggleShowOriginal} 
        className={clsx(
          "p-2 rounded-full transition-colors",
          showOriginal 
            ? "bg-accent text-button-text hover:bg-accent/90 hover:text-button-text" 
            : "bg-surface hover:bg-card-active text-text-primary"
        )} 
        title={showOriginal ? "Show Edited (.)" : "Show Original (.)"}
      >
        {showOriginal ? <EyeOff size={20} /> : <Eye size={20} />}
      </button>
      
      <button 
        onClick={onToggleFullScreen} 
        className={clsx(
          "p-2 rounded-full transition-colors",
          isFullScreenLoading 
            ? "bg-accent text-button-text hover:bg-accent/90 hover:text-button-text" 
            : "bg-surface hover:bg-card-active text-text-primary"
        )} 
        title="Toggle Fullscreen (F)" 
        disabled={isFullScreenLoading}
      >
        {isFullScreenLoading ? <Loader2 size={20} className="animate-spin" /> : <Maximize size={20} />}
      </button>
    </div>
  </div>
));

export default EditorToolbar;