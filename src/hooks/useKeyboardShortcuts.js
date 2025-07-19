import { useEffect } from 'react';

export const useKeyboardShortcuts = ({
  selectedImage,
  isViewLoading,
  sortedImageList,
  multiSelectedPaths,
  libraryActivePath,
  zoom,
  canUndo,
  canRedo,
  activeRightPanel,
  isFullScreen,
  aiTool,
  activeMaskId,
  customEscapeHandler,
  copiedFilePaths,
  handleImageSelect,
  setLibraryActivePath,
  setMultiSelectedPaths,
  handleRate,
  handleDeleteSelected,
  handleCopyAdjustments,
  handlePasteAdjustments,
  handlePasteFiles,
  setCopiedFilePaths,
  undo,
  redo,
  handleBackToLibrary,
  handleToggleFullScreen,
  setShowOriginal,
  handleRightPanelSelect,
  setIsWaveformVisible,
  handleZoomChange,
  setAiTool,
  setActiveMaskId,
}) => {
  useEffect(() => {
    const handleKeyDown = (event) => {
      const isInputFocused = document.activeElement.tagName === 'INPUT' || document.activeElement.tagName === 'TEXTAREA';
      if (isInputFocused) return;
      const isCtrl = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();

      if (selectedImage) {
        if (key === 'escape') {
          event.preventDefault();
          if (customEscapeHandler) {
            customEscapeHandler();
          } else if (aiTool) {
            setAiTool(null);
          } else if (activeMaskId) {
            setActiveMaskId(null);
          } else if (isFullScreen) {
            handleToggleFullScreen();
          } else {
            handleBackToLibrary();
          }
          return;
        }
        if (key === ' ' && !isCtrl) {
            event.preventDefault();
            if (Math.abs(zoom - 2) < 0.01) {
                handleZoomChange(1);
            } else {
                handleZoomChange(2);
            }
            return;
        }
        if (key === 'f' && !isCtrl) { event.preventDefault(); handleToggleFullScreen(); }
        if (key === 'b' && !isCtrl) { event.preventDefault(); setShowOriginal(prev => !prev); }
        if (key === 'r' && !isCtrl) { event.preventDefault(); handleRightPanelSelect('crop'); }
        if (key === 'm' && !isCtrl) { event.preventDefault(); handleRightPanelSelect('masks'); }
        if (key === 'i' && !isCtrl) { event.preventDefault(); handleRightPanelSelect('metadata'); }
        if (key === 'e' && !isCtrl) { event.preventDefault(); handleRightPanelSelect('export'); }
        if (key === 'w' && !isCtrl) { event.preventDefault(); setIsWaveformVisible(prev => !prev); }
      }

      if (['arrowup', 'arrowdown', 'arrowleft', 'arrowright'].includes(key)) {
        if (isViewLoading) { event.preventDefault(); return; }
        event.preventDefault();

        if (selectedImage) {
            if (key === 'arrowup' || key === 'arrowdown') {
                const zoomStep = 0.25;
                const newZoom = key === 'arrowup' ? zoom + zoomStep : zoom - zoomStep;
                const minZoom = activeRightPanel === 'crop' ? 0.4 : 0.7;
                handleZoomChange(Math.max(minZoom, Math.min(newZoom, 10)));
            } else {
                const isNext = key === 'arrowright';
                const currentIndex = sortedImageList.findIndex(img => img.path === selectedImage.path);
                if (currentIndex === -1) return;
                let nextIndex = isNext ? currentIndex + 1 : currentIndex - 1;
                if (nextIndex >= sortedImageList.length) nextIndex = 0;
                if (nextIndex < 0) nextIndex = sortedImageList.length - 1;
                const nextImage = sortedImageList[nextIndex];
                if (nextImage) handleImageSelect(nextImage.path);
            }
        } else {
            const isNext = key === 'arrowright' || key === 'arrowdown';
            const activePath = libraryActivePath;
            if (!activePath || sortedImageList.length === 0) return;
            const currentIndex = sortedImageList.findIndex(img => img.path === activePath);
            if (currentIndex === -1) return;
            let nextIndex = isNext ? currentIndex + 1 : currentIndex - 1;
            if (nextIndex >= sortedImageList.length) nextIndex = 0;
            if (nextIndex < 0) nextIndex = sortedImageList.length - 1;
            const nextImage = sortedImageList[nextIndex];
            if (nextImage) {
                setLibraryActivePath(nextImage.path);
                setMultiSelectedPaths([nextImage.path]);
            }
        }
      }

      if (['0', '1', '2', '3', '4', '5'].includes(key) && !isCtrl) { event.preventDefault(); handleRate(parseInt(key, 10)); }
      if (key === 'delete') { event.preventDefault(); handleDeleteSelected(); }

      if (isCtrl) {
        switch (key) {
          case 'c': event.preventDefault(); if (event.shiftKey) { if (multiSelectedPaths.length > 0) { setCopiedFilePaths(multiSelectedPaths); } } else handleCopyAdjustments(); break;
          case 'v': event.preventDefault(); if (event.shiftKey) handlePasteFiles('copy'); else handlePasteAdjustments(); break;
          case 'a': event.preventDefault(); if (sortedImageList.length > 0) { setMultiSelectedPaths(sortedImageList.map(f => f.path)); if (!selectedImage) setLibraryActivePath(sortedImageList[sortedImageList.length - 1].path); } break;
          case 'z': if (selectedImage) { event.preventDefault(); undo(); } break;
          case 'y': if (selectedImage) { event.preventDefault(); redo(); } break;
          default: break;
        }
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [ sortedImageList, selectedImage, undo, redo, isFullScreen, handleToggleFullScreen, handleBackToLibrary, handleRightPanelSelect, handleRate, handleDeleteSelected, handleCopyAdjustments, handlePasteAdjustments, multiSelectedPaths, copiedFilePaths, handlePasteFiles, libraryActivePath, handleImageSelect, zoom, handleZoomChange, customEscapeHandler, activeMaskId, aiTool, isViewLoading, activeRightPanel, canRedo, canUndo, setAiTool, setActiveMaskId, setCopiedFilePaths, setIsWaveformVisible, setLibraryActivePath, setMultiSelectedPaths, setShowOriginal ]);
};