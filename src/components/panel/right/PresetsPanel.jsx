import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import { usePresets } from '../../../hooks/usePresets';
import { useContextMenu } from '../../../context/ContextMenuContext';
import { Plus, Loader2, FileUp, FileDown, Edit, Trash2, CopyPlus, RefreshCw } from 'lucide-react';
import { motion, AnimatePresence } from 'framer-motion';
import AddPresetModal from '../../modals/AddPresetModal';
import RenamePresetModal from '../../modals/RenamePresetModal';
import { INITIAL_ADJUSTMENTS } from '../../../utils/adjustments';

const itemVariants = {
  hidden: { opacity: 0, x: -15 },
  visible: i => ({
    opacity: 1,
    x: 0,
    transition: {
      duration: 0.25,
      delay: i * 0.05,
    },
  }),
  exit: { opacity: 0, x: -15, transition: { duration: 0.2 } },
};

export default function PresetsPanel({ adjustments, setAdjustments, selectedImage, activePanel }) {
  const { 
    presets, 
    isLoading, 
    addPreset, 
    deletePreset, 
    renamePreset,
    updatePreset,
    duplicatePreset,
    importPresetsFromFile,
    exportPresetsToFile,
  } = usePresets(adjustments);

  const { showContextMenu } = useContextMenu();
  const [previews, setPreviews] = useState({});
  const [isGeneratingPreviews, setIsGeneratingPreviews] = useState(false);
  const [isAddModalOpen, setIsAddModalOpen] = useState(false);
  const [renameModalState, setRenameModalState] = useState({ isOpen: false, preset: null });

  const generatePreviews = useCallback(async () => {
    if (!selectedImage?.isReady || presets.length === 0) {
      setPreviews({});
      return;
    }
    
    setIsGeneratingPreviews(true);
    const newPreviews = {};

    const presetsToPreview = [...presets];

    for (const preset of presetsToPreview) {
      try {
        const fullPresetAdjustments = { ...INITIAL_ADJUSTMENTS, ...preset.adjustments };
        const previewUrl = await invoke('generate_preset_preview', { jsAdjustments: fullPresetAdjustments });
        newPreviews[preset.id] = previewUrl;
      } catch (error) {
        console.error(`Failed to generate preview for preset ${preset.name}:`, error);
        newPreviews[preset.id] = null;
      }
    }
    setPreviews(newPreviews);
    setIsGeneratingPreviews(false);
  }, [presets, selectedImage?.isReady]);

  useEffect(() => {
    if (activePanel === 'presets' && selectedImage?.isReady) {
      generatePreviews();
    } else {
      setPreviews({});
    }
  }, [activePanel, generatePreviews, selectedImage?.isReady]);


  const handleApplyPreset = (preset) => {
    setAdjustments(prevAdjustments => ({
      ...prevAdjustments,
      ...preset.adjustments,
    }));
  };

  const handleSaveCurrentSettingsAsPreset = (name) => {
    addPreset(name);
    setIsAddModalOpen(false);
  };

  const handleRenameSave = (newName) => {
    if (renameModalState.preset) {
      renamePreset(renameModalState.preset.id, newName);
    }
    setRenameModalState({ isOpen: false, preset: null });
  };

  const handleImportPresets = async () => {
    try {
      const selectedPath = await openDialog({
        multiple: false,
        filters: [{ name: 'Preset File', extensions: ['rrpreset'] }],
        title: 'Import Presets',
      });

      if (typeof selectedPath === 'string') {
        await importPresetsFromFile(selectedPath);
      }
    } catch (error) {
      console.error('Failed to import presets:', error);
    }
  };

  const handleExportPreset = async (presetToExport) => {
    try {
      const filePath = await saveDialog({
        defaultPath: `${presetToExport.name}.rrpreset`.replace(/[<>:"/\\|?*]/g, '_'),
        filters: [{ name: 'Preset File', extensions: ['rrpreset'] }],
        title: 'Export Preset',
      });

      if (filePath) {
        await exportPresetsToFile([presetToExport], filePath);
      }
    } catch (error) {
      console.error('Failed to export preset:', error);
    }
  };
  
  const handleExportAllPresets = async () => {
    if (presets.length === 0) {
      console.log("No presets to export.");
      return;
    }
    try {
      const filePath = await saveDialog({
        defaultPath: 'all_presets.rrpreset',
        filters: [{ name: 'Preset File', extensions: ['rrpreset'] }],
        title: 'Export All Presets',
      });

      if (filePath) {
        await exportPresetsToFile(presets, filePath);
      }
    } catch (error) {
      console.error('Failed to export all presets:', error);
    }
  };

  const handleContextMenu = (event, preset) => {
    event.preventDefault();
    event.stopPropagation();
    
    const options = [
      {
        label: 'Overwrite Preset',
        icon: RefreshCw,
        onClick: () => updatePreset(preset.id),
      },
      { type: 'separator' },
      {
        label: 'Rename Preset',
        icon: Edit,
        onClick: () => {
          setRenameModalState({ isOpen: true, preset: preset });
        },
      },
      {
        label: 'Duplicate Preset',
        icon: CopyPlus,
        onClick: () => duplicatePreset(preset.id),
      },
      {
        label: 'Export Preset',
        icon: FileDown,
        onClick: () => handleExportPreset(preset),
      },
      { type: 'separator' },
      {
        label: 'Delete Preset',
        icon: Trash2,
        isDestructive: true,
        onClick: () => deletePreset(preset.id),
      },
    ];
    
    showContextMenu(event.clientX, event.clientY, options);
  };

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 flex justify-between items-center flex-shrink-0 border-b border-surface">
        <h2 className="text-xl font-bold text-primary text-shadow-shiny">Presets</h2>
        <div className="flex items-center gap-1">
          <button 
            onClick={handleImportPresets}
            title="Import presets from .rrpreset file" 
            className="p-2 rounded-full hover:bg-surface transition-colors"
            disabled={isLoading}
          >
            <FileUp size={18} />
          </button>
          <button 
            onClick={handleExportAllPresets}
            title="Export all presets to .rrpreset file" 
            className="p-2 rounded-full hover:bg-surface transition-colors"
            disabled={presets.length === 0 || isLoading}
          >
            <FileDown size={18} />
          </button>
          <button 
            onClick={() => setIsAddModalOpen(true)} 
            title="Save current settings as new preset" 
            className="p-2 rounded-full hover:bg-surface transition-colors"
            disabled={isLoading}
          >
            <Plus size={18} />
          </button>
        </div>
      </div>

      <div className="flex-grow overflow-y-auto p-4">
        {isLoading && presets.length > 0 && (
          <div className="text-center text-text-secondary py-2">
            <Loader2 size={16} className="animate-spin inline-block mr-2" /> Updating...
          </div>
        )}
        {!isLoading && presets.length === 0 ? (
          <div className="text-center text-text-secondary py-8">
            No presets saved yet. Click the '+' button to save current settings as a preset, or import from a file.
          </div>
        ) : (
          <div className="space-y-2">
            <AnimatePresence>
              {[...presets].sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())).map((preset, index) => (
                <motion.div
                  key={preset.id}
                  layout
                  variants={itemVariants}
                  initial="hidden"
                  animate="visible"
                  exit="exit"
                  custom={index}
                  onClick={() => handleApplyPreset(preset)}
                  onContextMenu={(e) => handleContextMenu(e, preset)}
                  className="flex items-center gap-2 p-2 rounded-lg bg-surface cursor-pointer hover:bg-surface-hover"
                >
                  <div className="w-20 h-14 bg-bg-tertiary rounded-md flex items-center justify-center flex-shrink-0">
                    {isGeneratingPreviews && !previews[preset.id] ? (
                       <Loader2 size={20} className="animate-spin text-text-secondary" />
                    ) : previews[preset.id] ? (
                      <img src={previews[preset.id]} alt={`${preset.name} preview`} className="w-full h-full object-cover rounded-md" />
                    ) : (
                      <Loader2 size={20} className="animate-spin text-text-secondary" />
                    )}
                  </div>
                  <div className="flex-grow min-w-0">
                    <p className="font-medium truncate">{preset.name}</p>
                  </div>
                </motion.div>
              ))}
            </AnimatePresence>
          </div>
        )}
      </div>
      
      <AddPresetModal
        isOpen={isAddModalOpen}
        onClose={() => setIsAddModalOpen(false)}
        onSave={handleSaveCurrentSettingsAsPreset}
      />
      <RenamePresetModal
        isOpen={renameModalState.isOpen}
        onClose={() => setRenameModalState({ isOpen: false, preset: null })}
        onSave={handleRenameSave}
        currentName={renameModalState.preset?.name}
      />
    </div>
  );
}