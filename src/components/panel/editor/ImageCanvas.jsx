import { useState, useEffect, useRef, useCallback, memo, useMemo } from 'react';
import ReactCrop from 'react-image-crop';
import 'react-image-crop/dist/ReactCrop.css';
import { Stage, Layer, Ellipse, Line, Transformer, Group, Circle, Rect } from 'react-konva';
import Konva from 'konva';
import clsx from 'clsx';

function linesIntersect(eraserLine, drawnLine) {
  const threshold = (eraserLine.brushSize / 2) + (drawnLine.brushSize / 2);
  for (const p1 of eraserLine.points) {
    for (const p2 of drawnLine.points) {
      const distance = Math.sqrt(Math.pow(p1.x - p2.x, 2) + Math.pow(p1.y - p2.y, 2));
      if (distance < threshold) return true;
    }
  }
  return false;
}

const MaskOverlay = memo(({ subMask, scale, onUpdate, isSelected, onSelect, onMaskMouseEnter, onMaskMouseLeave, adjustments }) => {
  const shapeRef = useRef();
  const trRef = useRef();

  const crop = adjustments.crop;
  const cropX = crop ? crop.x : 0;
  const cropY = crop ? crop.y : 0;

  useEffect(() => {
    if (isSelected && trRef.current && shapeRef.current) {
      trRef.current.nodes([shapeRef.current]);
      trRef.current.getLayer().batchDraw();
    }
  }, [isSelected]);

  const handleRadialDrag = useCallback((e) => {
    onUpdate(subMask.id, {
      parameters: { 
        ...subMask.parameters, 
        centerX: (e.target.x() / scale) + cropX, 
        centerY: (e.target.y() / scale) + cropY 
      },
    });
  }, [subMask.id, subMask.parameters, onUpdate, scale, cropX, cropY]);

  const handleRadialTransform = useCallback(() => {
    const node = shapeRef.current;
    if (!node) return;

    onUpdate(subMask.id, {
      parameters: {
        ...subMask.parameters,
        centerX: (node.x() / scale) + cropX,
        centerY: (node.y() / scale) + cropY,
        radiusX: (node.radiusX() * node.scaleX()) / scale,
        radiusY: (node.radiusY() * node.scaleY()) / scale,
        rotation: node.rotation(),
      },
    });
  }, [subMask.id, subMask.parameters, onUpdate, scale, cropX, cropY]);

  const handleRadialTransformEnd = useCallback(() => {
    const node = shapeRef.current;
    if (!node) return;
    
    const scaleX = node.scaleX();
    const scaleY = node.scaleY();

    node.scaleX(1);
    node.scaleY(1);

    onUpdate(subMask.id, {
      parameters: {
        ...subMask.parameters,
        centerX: (node.x() / scale) + cropX,
        centerY: (node.y() / scale) + cropY,
        radiusX: (node.radiusX() * scaleX) / scale,
        radiusY: (node.radiusY() * scaleY) / scale,
        rotation: node.rotation(),
      },
    });
  }, [subMask.id, subMask.parameters, onUpdate, scale, cropX, cropY]);

  const handleGroupDragEnd = (e) => {
    const group = e.target;
    const { startX, startY, endX, endY } = subMask.parameters;
    const dx = endX - startX;
    const dy = endY - startY;
    const centerX = startX + dx / 2;
    const centerY = startY + dy / 2;
    const groupX = (centerX - cropX) * scale;
    const groupY = (centerY - cropY) * scale;
    const moveX = group.x() - groupX;
    const moveY = group.y() - groupY;
    onUpdate(subMask.id, {
      parameters: {
        ...subMask.parameters,
        startX: startX + moveX / scale,
        startY: startY + moveY / scale,
        endX: endX + moveX / scale,
        endY: endY + moveY / scale,
      }
    });
  };

  const handlePointDrag = (e, point) => {
    const stage = e.target.getStage();
    const pointerPos = stage.getPointerPosition();
    if (!pointerPos) return;

    const newX = (pointerPos.x / scale) + cropX;
    const newY = (pointerPos.y / scale) + cropY;

    const newParams = { ...subMask.parameters };
    if (point === 'start') {
      newParams.startX = newX;
      newParams.startY = newY;
    } else {
      newParams.endX = newX;
      newParams.endY = newY;
    }
    onUpdate(subMask.id, { parameters: newParams });
  };
  
  const handleRangeDrag = (e) => {
    const newRange = Math.abs(e.target.y() / scale);
    onUpdate(subMask.id, {
      parameters: { ...subMask.parameters, range: newRange }
    });
  };

  if (!subMask.visible) {
    return null;
  }

  const commonProps = {
    onClick: onSelect,
    onTap: onSelect,
    stroke: isSelected ? '#0ea5e9' : (subMask.mode === 'subtractive' ? '#f43f5e' : 'white'),
    strokeWidth: isSelected ? 3 : 2,
    strokeScaleEnabled: false,
    dash: [4, 4],
    opacity: isSelected ? 1 : 0.7,
  };

  if (subMask.type === 'ai-subject') {
    const { startX, startY, endX, endY } = subMask.parameters;
    if (endX > startX && endY > startY) {
      return (
        <Rect
          x={(startX - cropX) * scale}
          y={(startY - cropY) * scale}
          width={(endX - startX) * scale}
          height={(endY - startY) * scale}
          onMouseEnter={onMaskMouseEnter}
          onMouseLeave={onMaskMouseLeave}
          {...commonProps}
        />
      );
    }
    return null;
  }

  if (subMask.type === 'brush') {
    const { lines = [] } = subMask.parameters;
    return (
      <Group onClick={onSelect} onTap={onSelect}>
        {lines.map((line, i) => (
          <Line
            key={i}
            points={line.points.flatMap(p => [(p.x - cropX) * scale, (p.y - cropY) * scale])}
            stroke={isSelected ? '#0ea5e9' : (subMask.mode === 'subtractive' ? '#f43f5e' : 'white')}
            strokeWidth={isSelected ? 3 : 2}
            dash={[4, 4]}
            opacity={isSelected ? 1 : 0.7}
            hitStrokeWidth={line.brushSize * scale}
            strokeScaleEnabled={false}
            tension={0.5}
            lineCap="round"
            lineJoin="round"
          />
        ))}
      </Group>
    );
  }

  if (subMask.type === 'radial') {
    const { centerX, centerY, radiusX, radiusY, rotation } = subMask.parameters;
    return (
      <>
        <Ellipse
          ref={shapeRef}
          x={(centerX - cropX) * scale}
          y={(centerY - cropY) * scale}
          radiusX={radiusX * scale}
          radiusY={radiusY * scale}
          rotation={rotation}
          draggable
          onDragMove={handleRadialDrag}
          onDragEnd={handleRadialDrag}
          onTransform={handleRadialTransform}
          onTransformEnd={handleRadialTransformEnd}
          onMouseEnter={onMaskMouseEnter}
          onMouseLeave={onMaskMouseLeave}
          {...commonProps}
        />
        {isSelected && (
          <Transformer ref={trRef} boundBoxFunc={(oldBox, newBox) => newBox} onMouseEnter={onMaskMouseEnter} onMouseLeave={onMaskMouseLeave} />
        )}
      </>
    );
  }

  if (subMask.type === 'linear') {
    const { startX, startY, endX, endY, range = 50 } = subMask.parameters;
    const dx = endX - startX;
    const dy = endY - startY;
    const len = Math.sqrt(dx * dx + dy * dy);
    const angle = Math.atan2(dy, dx);
    const centerX = startX + dx / 2;
    const centerY = startY + dy / 2;
    const groupX = (centerX - cropX) * scale;
    const groupY = (centerY - cropY) * scale;
    const scaledLen = len * scale;
    const r = range * scale;

    const lineProps = {
      ...commonProps,
      strokeWidth: isSelected ? 2.5 : 2,
      dash: [6, 6],
      hitStrokeWidth: 20,
    };

    const perpendicularDragBoundFunc = function(pos) {
      const group = this.getParent();
      const transform = group.getAbsoluteTransform().copy();
      transform.invert();
      const localPos = transform.point(pos);
      const constrainedLocalPos = { x: 0, y: localPos.y };
      return group.getAbsoluteTransform().point(constrainedLocalPos);
    };

    return (
      <Group
        x={groupX}
        y={groupY}
        rotation={angle * 180 / Math.PI}
        draggable={isSelected}
        onDragEnd={handleGroupDragEnd}
        onMouseEnter={(e) => {
          onMaskMouseEnter();
          const stage = e.target.getStage();
          if (stage) stage.container().style.cursor = 'move';
        }}
        onMouseLeave={(e) => {
          onMaskMouseLeave();
          const stage = e.target.getStage();
          if (stage) stage.container().style.cursor = 'default';
        }}
        onClick={onSelect}
        onTap={onSelect}
      >
        <Line points={[-5000, 0, 5000, 0]} {...lineProps} dash={[2, 3]} />
        <Line
          y={-r}
          points={[-scaledLen / 2, 0, scaledLen / 2, 0]}
          {...lineProps}
          draggable={isSelected}
          onDragMove={handleRangeDrag}
          onDragEnd={(e) => { handleRangeDrag(e); e.cancelBubble = true; }}
          dragBoundFunc={perpendicularDragBoundFunc}
          onMouseEnter={(e) => { e.target.getStage().container().style.cursor = 'row-resize'; onMaskMouseEnter(); }}
          onMouseLeave={(e) => { e.target.getStage().container().style.cursor = 'move'; onMaskMouseLeave(); }}
        />
        <Line
          y={r}
          points={[-scaledLen / 2, 0, scaledLen / 2, 0]}
          {...lineProps}
          draggable={isSelected}
          onDragMove={handleRangeDrag}
          onDragEnd={(e) => { handleRangeDrag(e); e.cancelBubble = true; }}
          dragBoundFunc={perpendicularDragBoundFunc}
          onMouseEnter={(e) => { e.target.getStage().container().style.cursor = 'row-resize'; onMaskMouseEnter(); }}
          onMouseLeave={(e) => { e.target.getStage().container().style.cursor = 'move'; onMaskMouseLeave(); }}
        />
        {isSelected && (
          <>
            <Circle
              x={-scaledLen / 2}
              y={0}
              radius={8}
              fill="#0ea5e9"
              stroke="white"
              strokeWidth={2}
              draggable
              onDragMove={(e) => handlePointDrag(e, 'start')}
              onDragEnd={(e) => { handlePointDrag(e, 'start'); e.cancelBubble = true; }}
              onMouseEnter={(e) => { e.target.getStage().container().style.cursor = 'grab'; onMaskMouseEnter(); }}
              onMouseLeave={(e) => { e.target.getStage().container().style.cursor = 'move'; onMaskMouseLeave(); }}
            />
            <Circle
              x={scaledLen / 2}
              y={0}
              radius={8}
              fill="#0ea5e9"
              stroke="white"
              strokeWidth={2}
              draggable
              onDragMove={(e) => handlePointDrag(e, 'end')}
              onDragEnd={(e) => { handlePointDrag(e, 'end'); e.cancelBubble = true; }}
              onMouseEnter={(e) => { e.target.getStage().container().style.cursor = 'grab'; onMaskMouseEnter(); }}
              onMouseLeave={(e) => { e.target.getStage().container().style.cursor = 'move'; onMaskMouseLeave(); }}
            />
          </>
        )}
      </Group>
    );
  }
  return null;
});

const ImageCanvas = memo(({
  isCropping, crop, setCrop, handleCropComplete, adjustments, selectedImage,
  isMasking, imageRenderSize, showOriginal, finalPreviewUrl, isAdjusting,
  uncroppedAdjustedPreviewUrl, maskOverlayUrl,
  onSelectMask, activeMaskId, activeMaskContainerId,
  updateSubMask, setIsMaskHovered, isMaskControlHovered,
  brushSettings, onGenerateAiMask, aiTool, onAiMaskDrawingComplete
}) => {
  const [isCropViewVisible, setIsCropViewVisible] = useState(false);
  const imagePathRef = useRef(null);
  const latestEditedUrlRef = useRef(null);
  const [layers, setLayers] = useState([]);
  const cropImageRef = useRef(null);
  const prevTransformPropsRef = useRef(null);

  const isDrawing = useRef(false);
  const currentLine = useRef(null);
  const [previewLine, setPreviewLine] = useState(null);
  const [cursorPreview, setCursorPreview] = useState({ x: 0, y: 0, visible: false });

  const activeContainer = useMemo(() => 
    adjustments.masks.find(c => c.id === activeMaskContainerId), 
    [adjustments.masks, activeMaskContainerId]
  );
  
  const activeSubMask = useMemo(() => 
    activeContainer?.subMasks.find(m => m.id === activeMaskId), 
    [activeContainer, activeMaskId]
  );

  const isBrushActive = isMasking && activeSubMask?.type === 'brush';
  const isAiSubjectActive = isMasking && activeSubMask?.type === 'ai-subject';

  const isGenerativeReplaceActive = aiTool === 'generative-replace';

  const sortedSubMasks = useMemo(() => {
    if (!activeContainer) return [];
    const selectedMask = activeContainer.subMasks.find(m => m.id === activeMaskId);
    const otherMasks = activeContainer.subMasks.filter(m => m.id !== activeMaskId);
    return selectedMask ? [...otherMasks, selectedMask] : activeContainer.subMasks;
  }, [activeContainer, activeMaskId]);

  useEffect(() => {
    const { path: currentImagePath, originalUrl, thumbnailUrl } = selectedImage;
    const topLayer = layers[layers.length - 1];

    const imageChanged = currentImagePath !== imagePathRef.current;

    const rotationChanged = !imageChanged && prevTransformPropsRef.current && prevTransformPropsRef.current.rotation !== adjustments.rotation;
    const aspectRatioChanged = !imageChanged && prevTransformPropsRef.current && prevTransformPropsRef.current.aspectRatio !== adjustments.aspectRatio;

    if (imageChanged || rotationChanged || aspectRatioChanged) {
        imagePathRef.current = currentImagePath;
        prevTransformPropsRef.current = {
            rotation: adjustments.rotation,
            aspectRatio: adjustments.aspectRatio,
        };
        latestEditedUrlRef.current = null;

        const initialUrl = thumbnailUrl || originalUrl;
        if (initialUrl) {
            setLayers([{ id: initialUrl, url: initialUrl, opacity: 1 }]);
        } else {
            setLayers([]);
        }
        return;
    }

    if (showOriginal && topLayer?.id !== 'original') {
      setLayers(prev => [...prev, { id: 'original', url: originalUrl, opacity: 0 }]);
      return;
    }
    if (!showOriginal && topLayer?.id === 'original') {
      const urlToShow = latestEditedUrlRef.current || finalPreviewUrl || originalUrl || thumbnailUrl;
      if (urlToShow) {
          setLayers(prev => [...prev, { id: urlToShow, url: urlToShow, opacity: 0 }]);
      }
      return;
    }

    if (finalPreviewUrl && finalPreviewUrl !== latestEditedUrlRef.current) {
      latestEditedUrlRef.current = finalPreviewUrl;
      const img = new Image();
      img.src = finalPreviewUrl;
      img.onload = () => {
        if (img.src === latestEditedUrlRef.current) {
          if (layers.length === 0) {
            setLayers([{ id: img.src, url: img.src, opacity: 1 }]);
          } else {
            setLayers(prev => [...prev, { id: img.src, url: img.src, opacity: 0 }]);
          }
        }
      };
      return () => { img.onload = null; };
    }

    if (layers.length === 0 && !finalPreviewUrl) {
        const initialUrl = originalUrl || thumbnailUrl;
        if (initialUrl && initialUrl !== latestEditedUrlRef.current) {
            latestEditedUrlRef.current = initialUrl;
            setLayers([{ id: initialUrl, url: initialUrl, opacity: 1 }]);
        }
    }
  }, [selectedImage, finalPreviewUrl, showOriginal, layers, adjustments.rotation, adjustments.aspectRatio]);

  useEffect(() => {
    const layerToFadeIn = layers.find(l => l.opacity === 0);
    if (layerToFadeIn) {
      const timer = setTimeout(() => {
        setLayers(prev =>
          prev.map(l => (l.id === layerToFadeIn.id ? { ...l, opacity: 1 } : l))
        );
      }, 10);

      return () => clearTimeout(timer);
    }
  }, [layers]);

  const handleTransitionEnd = useCallback((finishedId) => {
    setLayers(prev => prev.length > 1 ? prev.filter(l => l.id === finishedId) : prev);
  }, []);

  useEffect(() => {
    if (isCropping) {
      const timer = setTimeout(() => setIsCropViewVisible(true), 10);
      return () => clearTimeout(timer);
    } else {
      setIsCropViewVisible(false);
    }
  }, [isCropping]);

  const handleMouseDown = useCallback((e) => {
    const toolActive = isGenerativeReplaceActive || isBrushActive || isAiSubjectActive;
    if (toolActive) {
      e.evt.preventDefault();
      isDrawing.current = true;
      const stage = e.target.getStage();
      const pos = stage.getPointerPosition();
      if (!pos) return;

      let toolType = 'brush';
      if (isGenerativeReplaceActive) toolType = 'generative-replace';
      else if (isAiSubjectActive) toolType = 'ai-selector';

      const newLine = {
        tool: toolType,
        brushSize: isBrushActive ? brushSettings.size : (isGenerativeReplaceActive ? 50 : 2),
        points: [pos]
      };
      currentLine.current = newLine;
      setPreviewLine(newLine);
    } else {
      if (e.target === e.target.getStage()) {
        onSelectMask(null);
      }
    }
  }, [isGenerativeReplaceActive, isBrushActive, isAiSubjectActive, brushSettings, onSelectMask]);

  const handleMouseMove = useCallback((e) => {
    const toolActive = isGenerativeReplaceActive || isBrushActive || isAiSubjectActive;
    if (toolActive) {
      const stage = e.target.getStage();
      const pos = stage.getPointerPosition();
      if (pos) {
        setCursorPreview({ x: pos.x, y: pos.y, visible: true });
      } else {
        setCursorPreview(p => ({ ...p, visible: false }));
      }
    }

    if (!isDrawing.current || !toolActive) return;
    
    const stage = e.target.getStage();
    const pos = stage.getPointerPosition();
    if (!pos) return;

    const updatedLine = {
      ...currentLine.current,
      points: [...currentLine.current.points, pos]
    };
    currentLine.current = updatedLine;
    setPreviewLine(updatedLine);
  }, [isGenerativeReplaceActive, isBrushActive, isAiSubjectActive]);

  const handleMouseUp = useCallback(() => {
    if (!isDrawing.current || !currentLine.current) return;
    
    const wasDrawing = isDrawing.current;
    isDrawing.current = false;
    const line = currentLine.current;
    currentLine.current = null;
    setPreviewLine(null);

    if (!wasDrawing || !line) return;

    const { scale } = imageRenderSize;
    const cropX = adjustments.crop?.x || 0;
    const cropY = adjustments.crop?.y || 0;

    if (isGenerativeReplaceActive) {
      if (line.points.length < 2) return;
      const tempStage = new Konva.Stage({ container: document.createElement('div'), width: selectedImage.width, height: selectedImage.height });
      const tempLayer = new Konva.Layer();
      tempStage.add(tempLayer);

      const transformedPoints = line.points.flatMap(p => [
        (p.x / scale) + cropX,
        (p.y / scale) + cropY
      ]);

      tempLayer.add(new Konva.Line({
        points: transformedPoints,
        stroke: 'white',
        strokeWidth: line.brushSize / scale,
        fill: 'white',
        lineCap: 'round',
        lineJoin: 'round',
        closed: true,
      }));
      
      const maskDataBase64 = tempLayer.toDataURL({ mimeType: 'image/png' });
      tempStage.destroy();
      onAiMaskDrawingComplete(maskDataBase64);
      return;
    }
    

    if (isAiSubjectActive) {
      const points = line.points;
      if (points.length > 1) {
        const xs = points.map(p => p.x);
        const ys = points.map(p => p.y);
        const minX = Math.min(...xs);
        const minY = Math.min(...ys);
        const maxX = Math.max(...xs);
        const maxY = Math.max(...ys);

        const startPoint = { x: minX / scale + cropX, y: minY / scale + cropY };
        const endPoint = { x: maxX / scale + cropX, y: maxY / scale + cropY };
        
        if (onGenerateAiMask) {
            onGenerateAiMask(activeMaskId, startPoint, endPoint);
        }
      }
    } else if (isBrushActive) {
      const imageSpaceLine = {
        tool: brushSettings.tool,
        brushSize: brushSettings.size / scale,
        feather: brushSettings.feather / 100,
        points: line.points.map(p => ({
          x: p.x / scale + cropX,
          y: p.y / scale + cropY,
        }))
      };

      const existingLines = activeSubMask.parameters.lines || [];

      if (brushSettings.tool === 'eraser') {
        const remainingLines = existingLines.filter(
          drawnLine => !linesIntersect(imageSpaceLine, drawnLine)
        );
        if (remainingLines.length !== existingLines.length) {
          updateSubMask(activeMaskId, {
            parameters: { ...activeSubMask.parameters, lines: remainingLines }
          });
        }
      } else {
        updateSubMask(activeMaskId, {
          parameters: {
            ...activeSubMask.parameters,
            lines: [...existingLines, imageSpaceLine]
          }
        });
      }
    }
  }, [isGenerativeReplaceActive, isBrushActive, isAiSubjectActive, activeSubMask, activeMaskId, updateSubMask, adjustments.crop, imageRenderSize.scale, brushSettings, onGenerateAiMask, onAiMaskDrawingComplete, selectedImage.width, selectedImage.height]);

  const handleMouseEnter = useCallback(() => {
    if (isGenerativeReplaceActive || isBrushActive || isAiSubjectActive) {
      setCursorPreview(p => ({ ...p, visible: true }));
    }
  }, [isGenerativeReplaceActive, isBrushActive, isAiSubjectActive]);

  const handleMouseLeave = useCallback(() => {
    setCursorPreview(p => ({ ...p, visible: false }));
    if (isDrawing.current) {
      handleMouseUp();
    }
  }, [handleMouseUp]);

  const cropPreviewUrl = uncroppedAdjustedPreviewUrl || selectedImage.originalUrl;
  const isContentReady = layers.length > 0 || selectedImage.thumbnailUrl;

  const uncroppedImageRenderSize = useMemo(() => {
    if (!selectedImage?.width || !selectedImage?.height || !imageRenderSize?.width || !imageRenderSize?.height) {
      return null;
    }

    const viewportWidth = imageRenderSize.width + 2 * imageRenderSize.offsetX;
    const viewportHeight = imageRenderSize.height + 2 * imageRenderSize.offsetY;

    let uncroppedEffectiveWidth = selectedImage.width;
    let uncroppedEffectiveHeight = selectedImage.height;
    const rotation = adjustments.rotation || 0;
    if (rotation === 90 || rotation === 270) {
        [uncroppedEffectiveWidth, uncroppedEffectiveHeight] = [uncroppedEffectiveHeight, uncroppedEffectiveWidth];
    }

    if (uncroppedEffectiveWidth <= 0 || uncroppedEffectiveHeight <= 0 || viewportWidth <= 0 || viewportHeight <= 0) {
        return null;
    }

    const scale = Math.min(
        viewportWidth / uncroppedEffectiveWidth,
        viewportHeight / uncroppedEffectiveHeight
    );

    const renderWidth = selectedImage.width * scale;
    const renderHeight = selectedImage.height * scale;

    return { width: renderWidth, height: renderHeight };
  }, [selectedImage?.width, selectedImage?.height, imageRenderSize, adjustments.rotation]);

  const cropImageTransforms = useMemo(() => {
    const transforms = [`rotate(${adjustments.rotation || 0}deg)`];
    if (adjustments.flipHorizontal) transforms.push('scaleX(-1)');
    if (adjustments.flipVertical) transforms.push('scaleY(-1)');
    return transforms.join(' ');
  }, [adjustments.rotation, adjustments.flipHorizontal, adjustments.flipVertical]);

  return (
    <div className="relative" style={{ width: '100%', height: '100%' }}>
      <div
        className="absolute inset-0 w-full h-full transition-opacity duration-200 flex items-center justify-center"
        style={{
          opacity: isCropViewVisible ? 0 : 1,
          pointerEvents: isCropViewVisible ? 'none' : 'auto',
        }}
      >
        <div
          className={clsx(
            "transition-opacity duration-300",
            isAdjusting && !showOriginal ? 'opacity-70' : 'opacity-100'
          )}
          style={{ 
            opacity: isContentReady ? 1 : 0,
            width: '100%',
            height: '100%',
            position: 'relative'
          }}
        >
          <div className="absolute inset-0 w-full h-full">
            {layers.map(layer => (
              <img
                key={layer.id}
                src={layer.url}
                alt={layer.id === 'original' ? 'Original' : 'Edited'}
                onTransitionEnd={() => handleTransitionEnd(layer.id)}
                className="absolute inset-0 w-full h-full object-contain"
                style={{
                  opacity: layer.opacity,
                  transition: 'opacity 150ms ease-in-out',
                }}
              />
            ))}
            {isMasking && maskOverlayUrl && (
              <img
                src={maskOverlayUrl}
                alt="Mask Overlay"
                className="absolute object-contain pointer-events-none"
                style={{
                  width: `${imageRenderSize.width}px`,
                  height: `${imageRenderSize.height}px`,
                  left: `${imageRenderSize.offsetX}px`,
                  top: `${imageRenderSize.offsetY}px`,
                  opacity: (showOriginal || isMaskControlHovered) ? 0 : 1,
                  transition: 'opacity 150ms ease-in-out',
                }}
              />
            )}
          </div>
        </div>

        <Stage
          width={imageRenderSize.width}
          height={imageRenderSize.height}
          className="transition-opacity duration-300"
          style={{
            position: 'absolute',
            left: `${imageRenderSize.offsetX}px`,
            top: `${imageRenderSize.offsetY}px`,
            zIndex: 4,
            opacity: showOriginal ? 0 : 1,
            pointerEvents: showOriginal ? 'none' : 'auto',
            cursor: isGenerativeReplaceActive ? 'none' : ((isBrushActive || isAiSubjectActive) ? 'crosshair' : 'default'),
          }}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
        >
          <Layer>
            {isMasking && activeContainer && sortedSubMasks.map(subMask => (
              <MaskOverlay
                key={subMask.id}
                subMask={subMask}
                scale={imageRenderSize.scale}
                onUpdate={updateSubMask}
                isSelected={subMask.id === activeMaskId}
                onSelect={() => onSelectMask(subMask.id)}
                onMaskMouseEnter={() => !(isBrushActive || isAiSubjectActive) && setIsMaskHovered(true)}
                onMaskMouseLeave={() => !(isBrushActive || isAiSubjectActive) && setIsMaskHovered(false)}
                adjustments={adjustments}
              />
            ))}
            {previewLine && (
              <Line
                points={previewLine.points.flatMap(p => [p.x, p.y])}
                stroke={
                  previewLine.tool === 'eraser' ? '#f43f5e' :
                  previewLine.tool === 'generative-replace' ? '#8b5cf6' :
                  '#0ea5e9'
                }
                strokeWidth={
                  previewLine.tool === 'ai-selector' ? 2 :
                  previewLine.tool === 'generative-replace' ? 3 :
                  previewLine.brushSize
                }
                tension={0.5}
                lineCap="round"
                lineJoin="round"
                opacity={0.8}
                listening={false}
                dash={
                  previewLine.tool === 'ai-selector' ? [4, 4] :
                  previewLine.tool === 'generative-replace' ? [6, 6] :
                  undefined
                }
              />
            )}
            {(isBrushActive || isGenerativeReplaceActive) && cursorPreview.visible && (
              <Circle
                x={cursorPreview.x}
                y={cursorPreview.y}
                radius={(isGenerativeReplaceActive ? 50 : brushSettings.size) / 2}
                stroke={
                  isGenerativeReplaceActive ? '#8b5cf6' :
                  brushSettings.tool === 'eraser' ? '#f43f5e' :
                  '#0ea5e9'
                }
                strokeWidth={isGenerativeReplaceActive ? 2 : 1}
                listening={false}
                perfectDrawEnabled={false}
              />
            )}
          </Layer>
        </Stage>
      </div>

      <div
        className="absolute inset-0 w-full h-full flex items-center justify-center transition-opacity duration-200"
        style={{
          opacity: isCropViewVisible ? 1 : 0,
          pointerEvents: isCropViewVisible ? 'auto' : 'none',
        }}
      >
        {cropPreviewUrl && uncroppedImageRenderSize && (
          <ReactCrop
            crop={crop}
            onChange={setCrop}
            onComplete={handleCropComplete}
            aspect={adjustments.aspectRatio}
            ruleOfThirds
          >
            <img
              ref={cropImageRef}
              alt="Crop preview"
              src={cropPreviewUrl}
              style={{ 
                display: 'block', 
                width: `${uncroppedImageRenderSize.width}px`,
                height: `${uncroppedImageRenderSize.height}px`,
                objectFit: 'contain',
                transform: cropImageTransforms,
              }}
            />
          </ReactCrop>
        )}
      </div>
    </div>
  );
});

export default ImageCanvas;