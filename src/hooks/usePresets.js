import { useState, useEffect, useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import debounce from 'lodash.debounce';
import { COPYABLE_ADJUSTMENT_KEYS } from '../utils/adjustments';

export function usePresets(currentAdjustments) {
  const [presets, setPresets] = useState([]);
  const [isLoading, setIsLoading] = useState(true);

  const loadPresets = useCallback(async () => {
    setIsLoading(true);
    try {
      const loadedPresets = await invoke('load_presets');
      setPresets(loadedPresets);
    } catch (error) {
      console.error('Failed to load presets:', error);
      setPresets([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const savePresetsToBackend = useCallback(debounce((presetsToSave) => {
    invoke('save_presets', { presets: presetsToSave })
      .catch(err => console.error("Failed to save presets:", err));
  }, 500), []);

  useEffect(() => {
    loadPresets();
  }, [loadPresets]);

  const addPreset = (name) => {
    const presetAdjustments = {};
    for (const key of COPYABLE_ADJUSTMENT_KEYS) {
      if (currentAdjustments.hasOwnProperty(key)) {
        presetAdjustments[key] = currentAdjustments[key];
      }
    }

    const newPreset = {
      id: crypto.randomUUID(),
      name,
      adjustments: presetAdjustments,
    };
    const updatedPresets = [...presets, newPreset];
    setPresets(updatedPresets);
    savePresetsToBackend(updatedPresets);
  };

  const deletePreset = (id) => {
    const updatedPresets = presets.filter(p => p.id !== id);
    setPresets(updatedPresets);
    savePresetsToBackend(updatedPresets);
  };

  const renamePreset = (id, newName) => {
    const updatedPresets = presets.map(p => 
      p.id === id ? { ...p, name: newName } : p
    );
    setPresets(updatedPresets);
    savePresetsToBackend(updatedPresets);
  };

  const updatePreset = (id) => {
    const presetAdjustments = {};
    for (const key of COPYABLE_ADJUSTMENT_KEYS) {
      if (currentAdjustments.hasOwnProperty(key)) {
        presetAdjustments[key] = currentAdjustments[key];
      }
    }

    const updatedPresets = presets.map(p => 
      p.id === id 
        ? { ...p, adjustments: presetAdjustments } 
        : p
    );
    setPresets(updatedPresets);
    savePresetsToBackend(updatedPresets);
  };

  const duplicatePreset = useCallback((presetId) => {
    const presetToDuplicate = presets.find(p => p.id === presetId);
    if (!presetToDuplicate) {
      console.error("Preset to duplicate not found");
      return;
    }

    const newPreset = {
      adjustments: JSON.parse(JSON.stringify(presetToDuplicate.adjustments)),
      id: crypto.randomUUID(),
      name: `${presetToDuplicate.name} Copy`,
    };

    const updatedPresets = [...presets, newPreset];
    setPresets(updatedPresets);
    savePresetsToBackend(updatedPresets);
  }, [presets, savePresetsToBackend]);

  const reorderPresets = (result) => {
    if (!result.destination) return;

    const items = Array.from(presets);
    const [reorderedItem] = items.splice(result.source.index, 1);
    items.splice(result.destination.index, 0, reorderedItem);

    setPresets(items);
    savePresetsToBackend(items);
  };

  const importPresetsFromFile = useCallback(async (filePath) => {
    setIsLoading(true);
    try {
      const updatedPresetList = await invoke('handle_import_presets_from_file', { filePath });
      setPresets(updatedPresetList);
    } catch (error) {
      console.error('Failed to import presets from file:', error);
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, [setPresets]);

  const exportPresetsToFile = useCallback(async (presetsToExport, filePath) => {
    try {
      await invoke('handle_export_presets_to_file', { presetsToExport, filePath });
    } catch (error) {
      console.error('Failed to export presets to file:', error);
      throw error;
    }
  }, []);

  return {
    presets,
    setPresets,
    isLoading,
    addPreset,
    deletePreset,
    renamePreset,
    updatePreset,
    duplicatePreset,
    reorderPresets,
    importPresetsFromFile,
    exportPresetsToFile,
    refreshPresets: loadPresets,
  };
}