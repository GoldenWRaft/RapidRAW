import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

function LutEditor() {
  const [originalImage, setOriginalImage] = useState(null);
  const [processedImage, setProcessedImage] = useState(null);
  const [lutFileContent, setLutFileContent] = useState(null);
  const [lutType, setLutType] = useState('cube'); // 'cube' or 'hald'
  const [isLoading, setIsLoading] = useState(false);

  const handleImageChange = (e) => {
    const file = e.target.files[0];
    if (file) {
      const reader = new FileReader();
      reader.onloadend = () => {
        setOriginalImage(reader.result);
      };
      reader.readAsDataURL(file);
    }
  };

  const handleLutFileChange = (e) => {
    const file = e.target.files[0];
    console.log('LUT File:', file);
    if (file) {
      const reader = new FileReader();
      let acceptedExtensions = ['.cube', '.3dl'];
      if (file.name.toLowerCase().endsWith('.cube') || file.name.toLowerCase().endsWith('.3dl')) {
        setLutType('cube');
        reader.onloadend = () => setLutFileContent(reader.result);
        reader.readAsText(file);
      } else { // Assume PNG, TIFF, etc. for Hald
        setLutType('hald');
        reader.onloadend = () => setLutFileContent(reader.result);
        reader.readAsDataURL(file);
      }
    }
  };

  const applyLut = async () => {
    console.log(originalImage, lutFileContent, lutType);
    return;

    if (originalImage && lutFileContent) {
      setIsLoading(true);
      setProcessedImage(null);
      try {
        const result = await invoke('apply_lut_type_gpu', {
          imageData: originalImage,
          lutData: lutFileContent,
          lutType: lutType,
        });
        setProcessedImage(result);
      } catch (error) {
        console.error('Error applying GPU LUT:', error);
        alert(`An error occurred: ${error}`);
      } finally {
        setIsLoading(false);
      }
    }
  };

  return (
    <div>
      <h2>GPU Accelerated LUT Module</h2>
      <div>
        <label>Select Image: </label>
        <input type="file" accept="image/*" onChange={handleImageChange} />
      </div>
      <div>
        <label>Select LUT File (.cube, .png for Hald): </label>
        <input type="file" accept=".cube,.png" onChange={handleLutFileChange} />
      </div>
      <button onClick={applyLut} disabled={!originalImage || !lutFileContent || isLoading}>
        {isLoading ? 'Processing...' : 'Apply LUT'}
      </button>
      <div style={{ display: 'flex', marginTop: '20px' }}>
        <div style={{ marginRight: '20px' }}>
          <h3>Original Image</h3>
          {originalImage && <img src={originalImage} alt="Original" width="400" />}
        </div>
        <div>
          <h3>Processed Image</h3>
          {processedImage && <img src={processedImage} alt="Processed" width="400" />}
        </div>
      </div>
    </div>
  );
}

export default LutEditor;