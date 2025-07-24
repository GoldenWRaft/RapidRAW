import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { TransformWrapper, TransformComponent } from "react-zoom-pan-pinch";
import { Loader2 } from 'lucide-react';
import clsx from 'clsx';
import { invoke } from '@tauri-apps/api/core';
import debounce from 'lodash.debounce';
import { AnimatePresence } from 'framer-motion';

import { useImageRenderSize } from '../../hooks/useImageRenderSize';

import FullScreenViewer from './editor/FullScreenViewer';
import EditorToolbar from './editor/EditorToolbar';
import ImageCanvas from './editor/ImageCanvas';
import Waveform from './editor/Waveform';

export default function Editor({
  selectedImage, finalPreviewUrl, uncroppedAdjustedPreviewUrl,
  showOriginal, setShowOriginal, isAdjusting, onBackToLibrary, isLoading, isFullScreen,
  isFullScreenLoading, fullScreenUrl, onToggleFullScreen, activeRightPanel,
  adjustments, setAdjustments, activeMaskId, activeMaskContainerId,
  onSelectMask, updateSubMask, transformWrapperRef, onZoomed, onContextMenu,
  onUndo, onRedo, canUndo, canRedo, brushSettings, 
  onGenerateAiMask, aiTool, onAiMaskDrawingComplete, isMaskControlHovered,
  targetZoom, waveform, isWaveformVisible, onCloseWaveform,
}) {
  const [crop, setCrop] = useState();
  const prevCropParams = useRef(null);
  const [isMaskHovered, setIsMaskHovered] = useState(false);
  const [isLoaderVisible, setIsLoaderVisible] = useState(false);
  const [maskOverlayUrl, setMaskOverlayUrl] = useState(null);
  const [transformState, setTransformState] = useState({ scale: 1, positionX: 0, positionY: 0 });
  const imageContainerRef = useRef(null);
  const isInitialMount = useRef(true);
  const transformStateRef = useRef(transformState);
  transformStateRef.current = transformState;

  useEffect(() => {
    if (!transformWrapperRef.current) {
      return;
    }

    const wrapperInstance = transformWrapperRef.current;
    const { zoomIn, zoomOut } = wrapperInstance;
    const currentScale = transformStateRef.current.scale;

    if (Math.abs(currentScale - targetZoom) < 0.001) {
      return;
    }

    const animationTime = 200;
    const animationType = 'easeOut';
    const factor = Math.log(targetZoom / currentScale);

    if (targetZoom > currentScale) {
      zoomIn(factor, animationTime, animationType);
    } else {
      zoomOut(-factor, animationTime, animationType);
    }
  }, [targetZoom, transformWrapperRef]);

  useEffect(() => {
    if (isInitialMount.current) {
      isInitialMount.current = false;
      return;
    }
    if (showOriginal) {
      setShowOriginal(false);
    }
  }, [finalPreviewUrl, setShowOriginal]);

  const isCropping = activeRightPanel === 'crop';
  const isMasking = activeRightPanel === 'masks';
  
  const hasDisplayableImage = finalPreviewUrl || selectedImage.originalUrl || selectedImage.thumbnailUrl;
  const showSpinner = isLoading && !hasDisplayableImage;

  const croppedDimensions = useMemo(() => {
    if (adjustments.crop) {
        return { width: adjustments.crop.width, height: adjustments.crop.height };
    }
    if (selectedImage) {
        const rotation = adjustments.rotation || 0;
        const isSwapped = Math.abs(rotation % 180) === 90;
        const width = isSwapped ? selectedImage.height : selectedImage.width;
        const height = isSwapped ? selectedImage.width : selectedImage.height;
        return { width, height };
    }
    return null;
  }, [selectedImage, adjustments.crop, adjustments.rotation]);

  const imageRenderSize = useImageRenderSize(imageContainerRef, croppedDimensions);

  const debouncedGenerateMaskOverlay = useCallback(debounce(async (maskDef, renderSize) => {
    if (!maskDef || !maskDef.visible || renderSize.width === 0) {
      setMaskOverlayUrl(null);
      return;
    }
    try {
      const cropOffset = [adjustments.crop?.x || 0, adjustments.crop?.y || 0];
      const url = await invoke('generate_mask_overlay', {
        maskDef,
        width: Math.round(renderSize.width),
        height: Math.round(renderSize.height),
        scale: renderSize.scale,
        cropOffset,
      });
      setMaskOverlayUrl(url);
    } catch (e) {
      console.error("Failed to generate mask overlay:", e);
      setMaskOverlayUrl(null);
    }
  }, 100), [adjustments.crop]);

  useEffect(() => {
    const activeContainer = activeMaskContainerId
      ? adjustments.masks.find(c => c.id === activeMaskContainerId)
      : null;

    debouncedGenerateMaskOverlay(activeContainer, imageRenderSize);
    
    return () => debouncedGenerateMaskOverlay.cancel();
  }, [activeMaskContainerId, adjustments.masks, imageRenderSize, debouncedGenerateMaskOverlay]);


  useEffect(() => {
    let timer;
    if (showSpinner) {
      setIsLoaderVisible(true);
    } else {
      timer = setTimeout(() => setIsLoaderVisible(false), 300);
    }
    return () => clearTimeout(timer);
  }, [showSpinner]);

  useEffect(() => {
    if (!isCropping || !selectedImage?.width) {
      return;
    }

    const { rotation = 0, aspectRatio } = adjustments;
    
    const hasChanged = prevCropParams.current?.rotation !== rotation || prevCropParams.current?.aspectRatio !== aspectRatio;

    if (hasChanged) {
      const { width: imgWidth, height: imgHeight } = selectedImage;
      const isSwapped = Math.abs(rotation % 180) === 90;
      const W = isSwapped ? imgHeight : imgWidth;
      const H = isSwapped ? imgWidth : imgHeight;
      const A = aspectRatio || W / H;
      if (isNaN(A)) return;

      const angle = Math.abs(rotation);
      const rad = (angle % 180) * Math.PI / 180;
      const sin = Math.sin(rad);
      const cos = Math.cos(rad);

      const h_c = Math.min(H / (A * sin + cos), W / (A * cos + sin));
      const w_c = A * h_c;

      const maxPixelCrop = {
        x: Math.round((W - w_c) / 2),
        y: Math.round((H - h_c) / 2),
        width: Math.round(w_c),
        height: Math.round(h_c),
      };
      
      prevCropParams.current = { rotation, aspectRatio };
      setAdjustments(prev => ({ ...prev, crop: maxPixelCrop }));
    }
  }, [isCropping, adjustments.rotation, adjustments.aspectRatio, selectedImage?.width, selectedImage?.height, setAdjustments]);


  useEffect(() => {
    if (!isCropping || !selectedImage?.width) {
      setCrop(undefined);
      return;
    }
    
    const rotation = adjustments.rotation || 0;
    const isSwapped = Math.abs(rotation % 180) === 90;
    const cropBaseWidth = isSwapped ? selectedImage.height : selectedImage.width;
    const cropBaseHeight = isSwapped ? selectedImage.width : selectedImage.height;

    const { crop: pixelCrop } = adjustments;

    if (pixelCrop) {
      setCrop({
        unit: '%',
        x: (pixelCrop.x / cropBaseWidth) * 100,
        y: (pixelCrop.y / cropBaseHeight) * 100,
        width: (pixelCrop.width / cropBaseWidth) * 100,
        height: (pixelCrop.height / cropBaseHeight) * 100,
      });
    } else {
      setCrop({ unit: '%', width: 100, height: 100, x: 0, y: 0 });
    }
  }, [isCropping, adjustments.crop, selectedImage]);

  const handleCropChange = useCallback((pixelCrop, percentCrop) => {
    setCrop(percentCrop);
  }, []);

  const handleCropComplete = useCallback((_, pc) => {
    if (!pc.width || !pc.height || !selectedImage?.width) return;

    const rotation = adjustments.rotation || 0;
    const isSwapped = Math.abs(rotation % 180) === 90;
    
    const cropBaseWidth = isSwapped ? selectedImage.height : selectedImage.width;
    const cropBaseHeight = isSwapped ? selectedImage.width : selectedImage.height;
    
    const newPixelCrop = {
      x: Math.round((pc.x / 100) * cropBaseWidth),
      y: Math.round((pc.y / 100) * cropBaseHeight),
      width: Math.round((pc.width / 100) * cropBaseWidth),
      height: Math.round((pc.height / 100) * cropBaseHeight),
    };

    if (JSON.stringify(newPixelCrop) !== JSON.stringify(adjustments.crop)) {
      setAdjustments(prev => ({ ...prev, crop: newPixelCrop }));
    }
  }, [selectedImage, adjustments.crop, adjustments.rotation, setAdjustments]);

  const toggleShowOriginal = useCallback(() => setShowOriginal(prev => !prev), [setShowOriginal]);

  const doubleClickProps = useMemo(() => {
    if (isCropping || isMasking) {
      return { 
        disabled: true,
      };
    }
    return {
      mode: transformState.scale >= 2 ? 'reset' : 'zoomIn',
      animationTime: 200,
      animationType: 'easeOut',
    };
  }, [isCropping, isMasking, transformState.scale]);  

  if (!selectedImage) {
    return (
      <div className="flex-1 bg-bg-secondary rounded-lg flex items-center justify-center text-text-secondary">
        <p>Select an image from the library to begin editing.</p>
      </div>
    );
  }

  const activeSubMask = useMemo(() => {
    if (!activeMaskId) return null;
    const container = adjustments.masks.find(c => c.subMasks.some(sm => sm.id === activeMaskId));
    return container?.subMasks.find(sm => sm.id === activeMaskId);
  }, [adjustments.masks, activeMaskId]);

  const isPanningDisabled = isMaskHovered || isCropping || aiTool === 'generative-replace' || (isMasking && (activeSubMask?.type === 'brush' || activeSubMask?.type === 'ai-subject'));

  return (
    <>
      <FullScreenViewer
        isOpen={isFullScreen}
        url={fullScreenUrl}
        onClose={onToggleFullScreen}
        thumbnailUrl={selectedImage.thumbnailUrl}
        transformState={transformState}
        onTransformChange={setTransformState}
      />

      <div className="flex-1 bg-bg-secondary rounded-lg flex flex-col relative overflow-hidden p-2 gap-2 min-h-0">
        <AnimatePresence>
          {isWaveformVisible && <Waveform waveformData={waveform} onClose={onCloseWaveform} />}
        </AnimatePresence>
        <EditorToolbar
          onBackToLibrary={onBackToLibrary}
          selectedImage={selectedImage}
          isLoading={isLoading}
          onToggleShowOriginal={toggleShowOriginal}
          showOriginal={showOriginal}
          onToggleFullScreen={onToggleFullScreen}
          isFullScreenLoading={isFullScreenLoading}
          onUndo={onUndo}
          onRedo={onRedo}
          canUndo={canUndo}
          canRedo={canRedo}
        />

        <div 
          className="flex-1 relative overflow-hidden rounded-lg" 
          ref={imageContainerRef}
          onContextMenu={onContextMenu}
        >
          {showSpinner && (
            <div className={clsx(
              "absolute inset-0 bg-bg-secondary/80 flex items-center justify-center z-50 transition-opacity duration-300",
              isLoaderVisible ? 'opacity-100' : 'opacity-0 pointer-events-none'
            )}>
              <Loader2 size={48} className="animate-spin text-accent" />
            </div>
          )}

          <TransformWrapper
            key={selectedImage.path}
            ref={transformWrapperRef}
            minScale={0.7}
            maxScale={10}
            limitToBounds={true}
            centerZoomedOut={true}
            doubleClick={doubleClickProps}
            panning={{ disabled: isPanningDisabled }}
            onTransformed={(_, state) => {
              setTransformState(state);
              onZoomed(state);
            }}
          >
            <TransformComponent wrapperStyle={{ width: '100%', height: '100%' }} contentStyle={{ width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
              <ImageCanvas
                isCropping={isCropping}
                crop={crop}
                setCrop={handleCropChange}
                handleCropComplete={handleCropComplete}
                adjustments={adjustments}
                selectedImage={selectedImage}
                isMasking={isMasking}
                imageRenderSize={imageRenderSize}
                showOriginal={showOriginal}
                finalPreviewUrl={finalPreviewUrl}
                isAdjusting={isAdjusting}
                uncroppedAdjustedPreviewUrl={uncroppedAdjustedPreviewUrl}
                maskOverlayUrl={maskOverlayUrl}
                onSelectMask={onSelectMask}
                activeMaskId={activeMaskId}
                activeMaskContainerId={activeMaskContainerId}
                updateSubMask={updateSubMask}
                isMaskHovered={isMaskHovered}
                setIsMaskHovered={setIsMaskHovered}
                brushSettings={brushSettings}
                onGenerateAiMask={onGenerateAiMask}
                aiTool={aiTool}
                onAiMaskDrawingComplete={onAiMaskDrawingComplete}
                isMaskControlHovered={isMaskControlHovered}
              />
            </TransformComponent>
          </TransformWrapper>
        </div>
      </div>
    </>
  );
}