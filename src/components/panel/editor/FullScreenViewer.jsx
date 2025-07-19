import { useState, useEffect, useCallback, useMemo, memo } from 'react';
import { TransformWrapper, TransformComponent } from "react-zoom-pan-pinch";
import { X } from 'lucide-react';
import clsx from 'clsx';

const FullScreenViewer = memo(({
  isOpen,
  onClose,
  url,
  thumbnailUrl,
  transformState,
  onTransformChange
}) => {
  const [isMounted, setIsMounted] = useState(false);
  const [show, setShow] = useState(false);
  const [isFullResLoaded, setIsFullResLoaded] = useState(false);

  useEffect(() => {
    if (isOpen) {
      setIsMounted(true);
      const timer = setTimeout(() => {
        setShow(true);
      }, 10);
      return () => clearTimeout(timer);
    } else {
      setShow(false);
      const timer = setTimeout(() => {
        setIsMounted(false);
      }, 300);
      return () => clearTimeout(timer);
    }
  }, [isOpen]);

  useEffect(() => {
    if (isOpen) {
      setIsFullResLoaded(false);
    }
  }, [isOpen, thumbnailUrl]);

  const handleClose = useCallback(() => {
    if (onClose) {
      onClose();
    }
  }, [onClose]);

  useEffect(() => {
    const handleKeyDown = (e) => {
      if (e.key === 'Escape' || e.key === 'f') {
        handleClose();
      }
    };

    if (isOpen) {
      window.addEventListener('keydown', handleKeyDown);
    }

    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [isOpen, handleClose]);

  const doubleClickProps = useMemo(() => {
    return {
      mode: transformState.scale >= 2 ? 'reset' : 'zoomIn',
      animationTime: 200,
      animationType: 'easeOut',
    };
  }, [transformState.scale]);  

  if (!isMounted) {
    return null;
  }

  return (
    <div
      className={clsx(
        'fixed inset-0 z-[100] flex items-center justify-center bg-black/60 backdrop-blur-sm transition-opacity duration-300 ease-in-out',
        show ? 'opacity-100' : 'opacity-0'
      )}
      onClick={handleClose}
      role="dialog"
      aria-modal="true"
    >
      <button
        className={clsx(
          'absolute top-4 right-4 text-white hover:text-gray-300 z-[102] transition-opacity duration-300',
          show ? 'opacity-100' : 'opacity-0'
        )}
        onClick={(e) => { e.stopPropagation(); handleClose(); }}
        title="Close (Esc or F)"
      >
        <X size={32} />
      </button>

      <div
        className={clsx(
          'w-full h-full flex items-center justify-center transform transition-all duration-300 ease-out',
          show ? 'scale-100 opacity-100 translate-y-0' : 'scale-95 opacity-0 -translate-y-4'
        )}
      >
        <TransformWrapper
          minScale={0.8}
          maxScale={10}
          limitToBounds={true}
          doubleClick={doubleClickProps}
          scale={transformState.scale}
          positionX={transformState.positionX}
          positionY={transformState.positionY}
          onTransformed={(_, state) => onTransformChange(state)}
        >
          <TransformComponent wrapperStyle={{ width: '100%', height: '100%' }} contentStyle={{ width: '100%', height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <div onClick={(e) => e.stopPropagation()}>
              <div className="relative">
                <img
                  src={thumbnailUrl}
                  alt="Preview"
                  className="w-auto h-[90vh] max-w-[95vw] object-contain"
                  onContextMenu={(e) => e.preventDefault()}
                />
                {url && (
                  <img
                    src={url}
                    alt="Fullscreen Preview"
                    onLoad={() => setIsFullResLoaded(true)}
                    className={clsx(
                      "absolute top-0 left-0 w-full h-full object-contain transition-opacity duration-300",
                      isFullResLoaded ? "opacity-100" : "opacity-0"
                    )}
                    onContextMenu={(e) => e.preventDefault()}
                  />
                )}
              </div>
            </div>
          </TransformComponent>
        </TransformWrapper>
      </div>
    </div>
  );
});

export default FullScreenViewer;