import { v4 as uuidv4 } from 'uuid';

export const createSubMask = (type, imageDimensions) => {
  const { width, height } = imageDimensions || { width: 1000, height: 1000 };
  const common = { id: uuidv4(), visible: true, mode: 'additive', type };

  switch (type) {
    case 'radial':
      return { ...common, parameters: { centerX: width / 2, centerY: height / 2, radiusX: width / 4, radiusY: width / 4, rotation: 0, feather: 0.5 } };
    case 'linear':
      return { ...common, parameters: { startX: width * 0.25, startY: height / 2, endX: width * 0.75, endY: height / 2, range: 50 } };
    case 'brush':
      return { ...common, parameters: { lines: [] } };
    case 'ai-subject':
      return { ...common, parameters: { maskDataBase64: null, grow: 0, feather: 0 } };
    case 'ai-foreground':
      return { ...common, parameters: { maskDataBase64: null, grow: 0, feather: 0 } };
    default:
      return { ...common, parameters: {} };
  }
};