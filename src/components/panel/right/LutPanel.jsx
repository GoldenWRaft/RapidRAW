import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useContextMenu } from '../../../context/ContextMenuContext';
import { Plus, RefreshCw, Star, X } from 'lucide-react';
import FolderTree from '../FolderTree';

// Helper function to get the file name from a path
const getLutNameFromPath = (path) => path.split(/[\\/]/).pop();

export default function LutPanel({ rootPath, selectedImage, activePanel, setFinalPreviewUrl }) {
      /*
        Processing of luts (Breakdown)
        - Open folders with luts and save the location
        - list out all the files in the folder with the correct extension
        - Allow favorite selection of luts
        - Allow user to select a lut and apply it to the image
        - Show files in a tree view 
        - Save settings for the applied lut for exporting later on

        🧠 Optionally Add:
          🔖 Favorites toggle per LUT file (store path in a useState or useLocalStorage) DONE 
          💾 Persist selected folders (store in a config file using Tauri)
          🌐 Search bar to filter through LUTs by name
          🎨 Preview thumbnails of each LUT applied to a small image
          🗂️ Group LUTs by subfolder or tags
          ⚡ Quick apply: double-click or right-click → "Apply LUT"
          ✨ Smooth transitions using Tailwind animation (transition, duration, etc.)
    */

  // --- STATE MANAGEMENT ---
  const [favoriteLuts, setFavoriteLuts] = useState([]); // Array of { path, name }
  const [recentLuts, setRecentLuts] = useState([]);     // Array of { path, name }
  
  const [selectedFolders, setSelectedFolders] = useState([]);
  const [folderTrees, setFolderTrees] = useState({});
  const [expandedFolders, setExpandedFolders] = useState(new Set());
  
  const [cachedImage, setCachedImage] = useState(null);
  const [lutApplied, setLutApplied] = useState(null);

  const { showContextMenu } = useContextMenu();

  // --- STATE PERSISTENCE (Loading and Saving) ---

useEffect(() => {
    const savedState = localStorage.getItem('lutAppState');
    if (savedState) {
      const state = JSON.parse(savedState);
      setFavoriteLuts(state.favoriteLuts || []);
      setRecentLuts(state.recentLuts || []);
      
      // FIX: Restore selected folders
      if (state.selectedFolders) {
        setSelectedFolders(state.selectedFolders);
      }
    }
  }, []);

  // Save state whenever favorites, recents, or selected folders change
  useEffect(() => {
    const state = {
      favoriteLuts,
      recentLuts,
      selectedFolders, // FIX: Add selectedFolders to the saved state
    };
    localStorage.setItem('lutAppState', JSON.stringify(state));
  }, [favoriteLuts, recentLuts, selectedFolders]);

  // FIX: Effect to re-load file trees when selectedFolders are restored
  useEffect(() => {
    const loadTrees = async () => {
      const newTrees = {};
      for (const folderPath of selectedFolders) {
        try {
          // Check if tree data already exists to avoid redundant calls
          if (!folderTrees[folderPath]) {
            const treeData = await invoke('get_file_tree', { path: folderPath });
            newTrees[folderPath] = treeData;
          }
        } catch (err) {
          console.error(`Failed to reload folder tree for ${folderPath}:`, err);
          // If a folder fails to load (e.g., deleted or on a disconnected drive),
          // remove it from the list to prevent errors.
          setSelectedFolders(prev => prev.filter(p => p !== folderPath));
        }
      }
      // Only update state if new trees were loaded
      if (Object.keys(newTrees).length > 0) {
        setFolderTrees(prev => ({ ...prev, ...newTrees }));
      }
    };

    if (selectedFolders.length > 0) {
      loadTrees();
    }
    // This effect should run when selectedFolders changes, but only for loading.
  }, [selectedFolders]);


  // --- CORE LUT FUNCTIONALITY ---

  const toggleFavorite = useCallback((lutItem) => {
    setFavoriteLuts(prev =>
      prev.find(f => f.path === lutItem.path)
        ? prev.filter(f => f.path !== lutItem.path) // Remove
        : [lutItem, ...prev]                        // Add
    );
  }, []);

  const addToRecent = useCallback((lutItem) => {
    setRecentLuts(prev => {
      const filtered = prev.filter(item => item.path !== lutItem.path);
      return [lutItem, ...filtered].slice(0, 5); // Keep max 10 recents
    });
  }, []);

  async function applyLut(lutPath) {
    if (lutApplied === null) {
      setCachedImage(selectedImage.originalUrl);
    }

    const lutName = getLutNameFromPath(lutPath);
    const lutType = lutPath.endsWith('.cube') || lutPath.endsWith('.3dl') ? 'cube' : 'hald';
    let success = false;

    try {
      let lutData;
      if (lutType === 'cube') {
        lutData = await invoke('read_file_data', { path: lutPath });
      } else {
        const rawData = await invoke('load_file_data', { path: lutPath });
        const pathEnd = lutPath.split('.').pop().toLowerCase();
        lutData = `data:image/${pathEnd};base64,${rawData}`;
      }
      
      const result = await invoke('apply_lut_type_gpu', {
        imageData: cachedImage || selectedImage.originalUrl,
        lutData: lutData,
        lutType: lutType,
      });

      setFinalPreviewUrl(result);
      setLutApplied({ path: lutPath, name: lutName });
      addToRecent({ path: lutPath, name: lutName });
      success = true;

    } catch (error) {
      console.error('Error applying LUT:', error);
      alert(`An error occurred while applying the LUT: ${error}`);
    }
    
    return success;
  }

  const resetLut = useCallback(() => {
    if (cachedImage) {
      selectedImage.originalUrl = cachedImage;
      setFinalPreviewUrl(cachedImage);
      setLutApplied(null);
      setCachedImage(null);
    }
  }, [cachedImage, selectedImage, setFinalPreviewUrl]);


  // --- UI HANDLERS ---

  const handleToggleFolder = useCallback((path) => {
    setExpandedFolders(prev => {
      const newSet = new Set(prev);
      newSet.has(path) ? newSet.delete(path) : newSet.add(path);
      return newSet;
    });
  }, []);

  async function selectFolder() {
    const outputFolder = await openDialog({
      title: `Select LUT Folder`,
      directory: true,
      multiple: false,
    });

    if (!outputFolder || selectedFolders.includes(outputFolder)) return;

    try {
      const treeData = await invoke('get_file_tree', { path: outputFolder });
      setSelectedFolders(prev => [...prev, outputFolder]);
      setFolderTrees(prev => ({ ...prev, [outputFolder]: treeData }));
    } catch (err) {
      console.error("Failed to load folder tree:", err);
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

  const handleNodeClick = useCallback((path, is_dir) => {
    if (is_dir) {
      handleToggleFolder(path);
    } else {
      applyLut(path);
    }
  }, [handleToggleFolder, applyLut]);
  
  const handleContextMenu = (event, path) => {
    event.preventDefault();

    if (!folderTrees || Object.keys(folderTrees).length === 0) {
      return; 
    }
    
    const node = Object.values(folderTrees).reduce((found, tree) => found || findNodeByPath(tree, path), null);

    if (!node || node.is_dir) return;

    const lutItem = { path, name: getLutNameFromPath(path) };
    const isFavorited = favoriteLuts.some(f => f.path === path);

    const menuOptions = [
      {
        label: isFavorited ? 'Remove from Favorites' : 'Add to Favorites',
        icon: Star,
        onClick: () => toggleFavorite(lutItem), 
      }
    ];
    
    showContextMenu(event.clientX, event.clientY, menuOptions);
  };

    const removeFolder = (folderPath) => {
    setSelectedFolders(prev => prev.filter(f => f !== folderPath));
    setFolderTrees(prev => {
        const newTrees = { ...prev };
        delete newTrees[folderPath];
        return newTrees;
    });
  }

  return (
    <div className="flex flex-col h-full bg-background-secondary">
      <div className="p-4 flex justify-between items-center flex-shrink-0 border-b border-surface">
        <h2 className="text-xl font-bold text-primary text-shadow-shiny">LUTs</h2>
        <button 
          onClick={selectFolder} 
          title="Add LUT Folder" 
          className="p-2 rounded-full hover:bg-surface transition-colors"
        >
          <Plus size={18} />
        </button>
      </div>

      <div className="flex-grow overflow-y-auto p-4 space-y-6">
        {/* --- APPLIED LUT INFO --- */}
        {lutApplied && (
          <div className="p-2 bg-surface rounded-md">
            <div className="flex items-center gap-2 justify-between" title={lutApplied.path}>
              <span className="text-sm text-text-secondary truncate">Applied:</span>
              <span className="text-sm font-semibold text-primary truncate">{lutApplied.name}</span>
              <button onClick={resetLut} className="p-1 rounded hover:bg-background" title="Reset LUT">
                <RefreshCw size={16} className="text-text-secondary" />
              </button>
            </div>
          </div>
        )}

        {/* --- FAVORITES SECTION --- */}
        <div>
          <h3 className="text-lg font-semibold text-primary mb-2">Favorites</h3>
          <div className="space-y-1">
            {favoriteLuts.length > 0 ? (
              favoriteLuts.map((lut) => (
                <div key={lut.path} className="flex items-center justify-between p-2 rounded-md hover:bg-surface group">
                  <span className="truncate cursor-pointer" onClick={() => applyLut(lut.path)} title={`Apply ${lut.name}`}>
                    {lut.name}
                  </span>
                  <button onClick={() => toggleFavorite(lut)} className="opacity-0 group-hover:opacity-100 transition-opacity" title="Remove from Favorites">
                    <X size={16} className="text-red-500"/>
                  </button>
                </div>
              ))
            ) : <p className="text-sm text-text-secondary px-2">No favorites yet. Right-click a LUT to add it.</p>}
          </div>
        </div>

        {/* --- RECENTS SECTION --- */}
        <div>
          <h3 className="text-lg font-semibold text-primary mb-2">Recently Used</h3>
          <div className="space-y-1">
            {recentLuts.length > 0 ? (
              recentLuts.map((lut) => (
                <div key={lut.path} className="p-2 rounded-md hover:bg-surface" onClick={() => applyLut(lut.path)} title={`Apply ${lut.name}`}>
                  <span className="truncate cursor-pointer">{lut.name}</span>
                </div>
              ))
            ) : <p className="text-sm text-text-secondary px-2">No recently used LUTs.</p>}
          </div>
        </div>

        {/* --- FOLDER TREE SECTION --- */}
        <div className="space-y-4">
          {selectedFolders.map((folderPath) => (
            <div key={folderPath}
                className="w-full rounded-md bg-muted/30 p-2 flex justify-between gap-2"
                title={folderPath}
              >
                <div>
                  <button
                    onClick={() => removeFolder(folderPath)}
                    className="text-xs bg-red-500 hover:bg-red-700 w-4 h-4 rounded-full mt-5" >
                  </button>
                </div>
                
              <FolderTree
                tree={folderTrees[folderPath]}
                onFolderSelect={(path) => {
                  const node = findNodeByPath(folderTrees[folderPath], path);
                  if (node) handleNodeClick(path, node.is_dir);
                }}
                onContextMenu={handleContextMenu}
                expandedFolders={expandedFolders}
                onToggleFolder={handleToggleFolder}
                isVisible={activePanel === 'lut'}
                style={{width: '100%'}}
                setIsVisible={() => {}}
                fileTree={true}
              />
            </div>
          ))}
          {selectedFolders.length === 0 && (
             <div className="text-center text-text-secondary py-10">
              <p>No LUT folders selected.</p>
              <p>Click the '+' button to add a folder.</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}