import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { usePresets } from '../../../hooks/usePresets';
import { useContextMenu } from '../../../context/ContextMenuContext';
import { Plus, Loader2, FileUp, FileDown, Edit, Trash2, CopyPlus, RefreshCw } from 'lucide-react';
import FolderTree from '../FolderTree';

export default function LutPanel({ rootPath, selectedImage, activePanel, setFinalPreviewUrl }) {
    /*
        Processing of luts (Breakdown)
        - Open folders with luts and save the location
        - list out all the files in the folder with the correct extension
        - Allow favorite selection of luts
        - Allow user to select a lut and apply it to the image
        - Show files in a tree view 
        - Save settings for the applied lut for exporting later on

    */

  const [selectedFolders, setSelectedFolders] = useState([]);
  const [folderTree, setFolderTree] = useState();
  const [expandedFolders, setExpandedFolders] = useState(new Set());
  const [cachedImage, setCachedImage] = useState(null);
  const [lutApplied, setLutApplied] = useState(false);
  

  const handleToggleFolder = useCallback((path) => {
    setExpandedFolders(prev => {
      const newSet = new Set(prev);
      if (newSet.has(path)) {
        newSet.delete(path);
      } else {
        newSet.add(path);
      }
      return newSet;
    });
  }, []);  

  async function selectFolder(){
    const outputFolder = await openDialog({
        title: `Select LUT Folder`,
        directory: true,
        multiple: false,
        canCreateDirectories: false
      });

    if (outputFolder) {
        setSelectedFolders(prev => [...prev, outputFolder]);
    }

     try {
        const treeData = await invoke('get_file_tree', { path: outputFolder });
        setFolderTree(treeData);
    } catch (err) {
        console.error("Failed to load folder tree:", err);
        setError(`Failed to load folder tree: ${err}. Some sub-folders might be inaccessible.`);
    }
  }

  function findNodeByPath(node, path) {
  if (!node) return null;
  if (node.path === path) return node;
  if (node.children) {
    for (const child of node.children) {
      const result = findNodeByPath(child, path);
      if (result) return result;
    }
  }
  return null;
}

const handleClick = useCallback(async (e, path, is_dir) => {
    if (lutApplied) {
      selectedImage.originalUrl = cachedImage;
    }
    else {
      setCachedImage(selectedImage.originalUrl);
    }

    if (!is_dir) {
      const reader = new FileReader();
      let lutType = path.endsWith('.cube') || path.endsWith('.3dl') ? 'cube' : 'hald';
      // read lut as text when lutType is cube
      if (lutType === 'cube') {
        let lutData = await invoke('read_file_data', { path });        
        await applyLutToImage(lutData, selectedImage.originalUrl, lutType);
      } else {
        let lutData = await invoke('load_file_data', { path });
        let pathEnd = path.split('.').pop().toLowerCase();
        let url = `data:image/${pathEnd};base64,${lutData}`;
        await applyLutToImage(url, selectedImage.originalUrl, lutType);
      }
    } else {
    handleToggleFolder(path);
  }
}, [selectedImage, handleToggleFolder]);

  async function applyLutToImage(lut_data, image_data, lutType) {
      if (lut_data) {
        try {
          const result = await invoke('apply_lut_type_gpu', {
            imageData: image_data,
            lutData: lut_data,
            lutType: lutType,
          });
          let imageUrl = URL.createObjectURL(new Blob([result], { type: 'image/png' }));

          // Handle the processed image result
          setFinalPreviewUrl(result);

          setLutApplied(true);
        } catch (error) {
          console.error('Error applying LUT:', error);
          alert(`An error occurred while applying the LUT: ${error}`);
        }
      }
    }

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 flex justify-between items-center flex-shrink-0 border-b border-surface">
        <h2 className="text-xl font-bold text-primary text-shadow-shiny">LUT Processing</h2>
        <div className="flex items-center gap-1">
          <button 
            onClick={() => selectFolder()} 
            title="Add LUT Folder" 
            className="p-2 rounded-full hover:bg-surface transition-colors"
          >
            <Plus size={18} />
          </button>
        </div>
      </div>

      <div className="flex-grow overflow-y-auto p-4">
        {folderTree ? (
            <FolderTree
                tree={folderTree}
                onFolderSelect={(path) => {
                  const node = findNodeByPath(folderTree, path);
                  if (!node) {
                    console.warn("Clicked path not found in tree:", path);
                    return;
                  }
                  handleClick(null, path, node.is_dir); // pass the expected args
                }}
                selectedPath={selectedFolders[0]}
                isVisible={activePanel === 'lut'}
                setIsVisible={() => {}}
                style={{ width: '100%' }}
                isResizing={false}
                onContextMenu={(e, path) => null}
                expandedFolders={expandedFolders}
                onToggleFolder={handleToggleFolder}
                fileTree={true}
            />
        ) : 
        <div className="text-center text-text-secondary">
          <p>No LUT folders selected. Please add a folder to start.</p>
        </div>
        }
      </div>
      
      {/* <AddPresetModal
        isOpen={isAddModalOpen}
        onClose={() => setIsAddModalOpen(false)}
        onSave={handleSaveCurrentSettingsAsPreset}
      />
      <RenamePresetModal
        isOpen={renameModalState.isOpen}
        onClose={() => setRenameModalState({ isOpen: false, preset: null })}
        onSave={handleRenameSave}
        currentName={renameModalState.preset?.name}
      /> */}
    </div>
  );
}