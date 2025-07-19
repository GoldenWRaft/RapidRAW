import { useState } from 'react';
import { ArrowLeft, Trash2, Wifi, WifiOff } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { relaunch } from '@tauri-apps/plugin-process';
import Button from '../ui/Button';
import ConfirmModal from '../modals/ConfirmModal';
import Dropdown from '../ui/Dropdown';
import Switch from '../ui/Switch';
import Input from '../ui/Input';
import { THEMES, DEFAULT_THEME_ID } from '../../utils/themes';

const resolutions = [
  { value: 720, label: '720px' },
  { value: 1280, label: '1280px' },
  { value: 1920, label: '1920px' },
  { value: 2560, label: '2560px' },
  { value: 3840, label: '3840px' },
];

const KeybindItem = ({ keys, description }) => (
  <div className="flex justify-between items-center py-2">
    <span className="text-text-secondary">{description}</span>
    <div className="flex items-center gap-1">
      {keys.map((key, index) => (
        <kbd key={index} className="px-2 py-1 text-xs font-sans font-semibold text-text-primary bg-bg-primary border border-border-color rounded-md">
          {key}
        </kbd>
      ))}
    </div>
  </div>
);

export default function SettingsPanel({ onBack, appSettings, onSettingsChange, rootPath, onLibraryRefresh }) {
  const [isClearing, setIsClearing] = useState(false);
  const [clearMessage, setClearMessage] = useState('');

  const [isClearingCache, setIsClearingCache] = useState(false);
  const [cacheClearMessage, setCacheClearMessage] = useState('');

  const [confirmModalState, setConfirmModalState] = useState({
    isOpen: false,
    title: '',
    message: '',
    onConfirm: () => {},
    confirmText: 'Confirm',
    confirmVariant: 'primary',
  });

  const [testStatus, setTestStatus] = useState({
    testing: false,
    message: '',
    success: null,
  });

  const effectiveRootPath = rootPath || appSettings?.lastRootPath;

  const executeClearSidecars = async () => {
    setIsClearing(true);
    setClearMessage('Deleting sidecar files, please wait...');
    try {
      const count = await invoke('clear_all_sidecars', { rootPath: effectiveRootPath });
      setClearMessage(`${count} sidecar files deleted successfully.`);
      onLibraryRefresh();
    } catch (err) {
      console.error("Failed to clear sidecars:", err);
      setClearMessage(`Error: ${err}`);
    } finally {
      setTimeout(() => {
        setIsClearing(false);
        setClearMessage('');
      }, 3000);
    }
  };

  const handleClearSidecars = () => {
    setConfirmModalState({
      isOpen: true,
      title: 'Confirm Deletion',
      message: 'Are you sure you want to delete all sidecar files?\n\nThis will permanently remove all your edits for all images inside the current root folder and its subfolders.',
      onConfirm: executeClearSidecars,
      confirmText: 'Delete All Edits',
      confirmVariant: 'destructive',
    });
  };

  const executeSetTransparent = async (transparent) => {
    onSettingsChange({ ...appSettings, transparent });
    await relaunch();
  };

  const handleSetTransparent = (transparent) => {
    setConfirmModalState({
      isOpen: true,
      title: 'Confirm Window Transparency',
        message: `Are you sure you want to ${ transparent ? 'enable' : 'disable' } window transparency effects?\n\n${ transparent ? 'These effects may reduce application performance.' : ''}\n\nThe application will relaunch to make this change.`,
      onConfirm: () => executeSetTransparent(transparent),
      confirmText: 'Toggle Transparency',
      confirmVariant: 'primary',
    });
  };

  const executeClearCache = async () => {
    setIsClearingCache(true);
    setCacheClearMessage('Clearing thumbnail cache...');
    try {
      await invoke('clear_thumbnail_cache');
      setCacheClearMessage('Thumbnail cache cleared successfully.');
      onLibraryRefresh();
    } catch (err) {
      console.error("Failed to clear thumbnail cache:", err);
      setCacheClearMessage(`Error: ${err}`);
    } finally {
      setTimeout(() => {
        setIsClearingCache(false);
        setCacheClearMessage('');
      }, 3000);
    }
  };

  const handleClearCache = () => {
    setConfirmModalState({
      isOpen: true,
      title: 'Confirm Cache Deletion',
      message: 'Are you sure you want to clear the thumbnail cache?\n\nAll thumbnails will need to be regenerated, which may be slow for large folders.',
      onConfirm: executeClearCache,
      confirmText: 'Clear Cache',
      confirmVariant: 'destructive',
    });
  };

  const handleTestConnection = async () => {
    if (!appSettings?.comfyuiAddress) return;
    setTestStatus({ testing: true, message: 'Testing...', success: null });
    try {
      await invoke('test_comfyui_connection', { address: appSettings.comfyuiAddress });
      setTestStatus({ testing: false, message: 'Connection successful!', success: true });
    } catch (err) {
      setTestStatus({ testing: false, message: `Connection failed.`, success: false });
      console.error("ComfyUI connection test failed:", err);
    } finally {
      setTimeout(() => setTestStatus({ testing: false, message: '', success: null }), 3000);
    }
  };

  const closeConfirmModal = () => {
    setConfirmModalState({ ...confirmModalState, isOpen: false });
  };

  return (
    <>
      <ConfirmModal
        {...confirmModalState}
        onClose={closeConfirmModal}
      />
      <div className="flex flex-col h-full w-full text-text-primary">
        <header className="flex-shrink-0 flex items-center mb-6">
          <Button onClick={onBack} variant="ghost" size="icon" className="mr-4 hover:bg-surface text-text-primary rounded-full">
            <ArrowLeft />
          </Button>
          <h1 className="text-3xl font-bold text-accent">Settings</h1>
        </header>
        <div className="flex-1 overflow-y-auto pr-2 -mr-2 space-y-8">
          
          <div className="p-6 bg-surface rounded-xl shadow-md">
            <h2 className="text-xl font-semibold mb-4 text-accent">General Settings</h2>
            <div className="space-y-4">
              <div>
                <label htmlFor="theme-select" className="block text-sm font-medium text-text-primary mb-2">
                  Theme
                </label>
                <Dropdown
                  options={THEMES.map(theme => ({ value: theme.id, label: theme.name }))}
                  value={appSettings?.theme || DEFAULT_THEME_ID}
                  onChange={(value) => onSettingsChange({ ...appSettings, theme: value })}
                />
                <p className="text-xs text-text-secondary mt-2">
                  Change the look and feel of the application.
                </p>
              </div>

              <div>
                <label htmlFor="adaptive-theme-toggle" className="block text-sm font-medium text-text-primary mb-2">
                  Editor Theme
                </label>
                <Switch
                  id="adaptive-theme-toggle"
                  label="Adaptive Editor Theme"
                  checked={appSettings?.adaptiveEditorTheme ?? false}
                  onChange={(checked) => onSettingsChange({ ...appSettings, adaptiveEditorTheme: checked })}
                />
                <p className="text-xs text-text-secondary mt-2">
                  Dynamically changes editor colors based on the current image.
                </p>
              </div>

              <div>
                <label htmlFor="window-effects-toggle" className="block text-sm font-medium text-text-primary mb-2">
                  Window Effects
                </label>
                <Switch
                  id="window-effects-toggle"
                  label="Transparency"
                  checked={appSettings?.transparent ?? true}
                  onChange={handleSetTransparent}
                />
              </div>

              <div className="pt-6 mt-6 border-t border-border-color">
                <label htmlFor="preview-resolution" className="block text-sm font-medium text-text-primary mb-2">
                  Preview Resolution
                </label>
                <Dropdown
                  options={resolutions}
                  value={appSettings?.editorPreviewResolution || 1920}
                  onChange={(value) => onSettingsChange({ ...appSettings, editorPreviewResolution: value })}
                />
                <p className="text-xs text-text-secondary mt-2">
                  Higher resolutions provide a sharper preview but may impact performance on less powerful systems.
                </p>
              </div>
            </div>
          </div>

          <div className="p-6 bg-surface rounded-xl shadow-md">
            <h2 className="text-xl font-semibold mb-4 text-accent">Integrations</h2>
            <div>
              <label htmlFor="comfyui-address" className="block text-sm font-medium text-text-primary mb-2">
                ComfyUI Address
              </label>
              <div className="flex items-center gap-2">
                <Input
                  id="comfyui-address"
                  type="text"
                  placeholder="127.0.0.1:8188"
                  value={appSettings?.comfyuiAddress || ''}
                  onChange={(e) => onSettingsChange({ ...appSettings, comfyuiAddress: e.target.value })}
                  className="flex-grow"
                />
                <Button
                  onClick={handleTestConnection}
                  disabled={testStatus.testing || !appSettings?.comfyuiAddress}
                  className="w-32"
                >
                  {testStatus.testing ? 'Testing...' : 'Test'}
                </Button>
              </div>
              {testStatus.message && (
                <p className={`text-sm mt-2 flex items-center gap-2 ${testStatus.success ? 'text-green-400' : 'text-red-400'}`}>
                  {testStatus.success === true && <Wifi size={16} />}
                  {testStatus.success === false && <WifiOff size={16} />}
                  {testStatus.message}
                </p>
              )}
              <p className="text-xs text-text-secondary mt-2">
                Enter the address and port of your running ComfyUI instance. Required for generative AI features.
              </p>
            </div>
          </div>

          <div className="p-6 bg-surface rounded-xl shadow-md">
            <h2 className="text-xl font-semibold mb-4 text-accent">Data Management</h2>
            <div className="space-y-4">
              <div>
                <h3 className="font-medium text-text-primary">Clear All Sidecar Files</h3>
                <p className="text-xs text-text-secondary mt-1 mb-3">
                  This will delete all `.rrdata` files (containing your edits) within the current root folder:
                  <span className="block font-mono text-xs bg-bg-primary p-2 rounded mt-1 break-all">
                    {effectiveRootPath || 'No folder selected'}
                  </span>
                </p>
                <Button
                  variant="destructive"
                  onClick={handleClearSidecars}
                  disabled={isClearing || !effectiveRootPath}
                >
                  <Trash2 size={16} className="mr-2" />
                  {isClearing ? 'Clearing...' : 'Delete All Edits in Folder'}
                </Button>
                {clearMessage && (
                  <p className="text-sm text-accent mt-3">{clearMessage}</p>
                )}
              </div>
              <div className="pt-6 border-t border-border-color">
                <h3 className="font-medium text-text-primary">Clear Thumbnail Cache</h3>
                <p className="text-xs text-text-secondary mt-1 mb-3">
                  This will delete all cached thumbnail images. They will be regenerated automatically as you browse your library.
                </p>
                <Button
                  variant="destructive"
                  onClick={handleClearCache}
                  disabled={isClearingCache}
                >
                  <Trash2 size={16} className="mr-2" />
                  {isClearingCache ? 'Clearing...' : 'Clear Thumbnail Cache'}
                </Button>
                {cacheClearMessage && (
                  <p className="text-sm text-accent mt-3">{cacheClearMessage}</p>
                )}
              </div>
            </div>
          </div>

          <div className="p-6 bg-surface rounded-xl shadow-md">
            <h2 className="text-xl font-semibold mb-4 text-accent">Keyboard Shortcuts</h2>
            <div className="space-y-1 text-sm divide-y divide-border-color">
              <div>
                <h3 className="text-lg font-semibold pt-3 pb-2 text-accent">General</h3>
                <KeybindItem keys={['Ctrl/Cmd', '+', 'C']} description="Copy selected adjustments" />
                <KeybindItem keys={['Ctrl/Cmd', '+', 'V']} description="Paste copied adjustments" />
                <KeybindItem keys={['Ctrl/Cmd', '+', 'Shift', '+', 'C']} description="Copy selected file(s)" />
                <KeybindItem keys={['Ctrl/Cmd', '+', 'Shift', '+', 'V']} description="Paste file(s) to current folder" />
                <KeybindItem keys={['Ctrl/Cmd', '+', 'A']} description="Select all images" />
                <KeybindItem keys={['Delete']} description="Delete selected file(s)" />
                <KeybindItem keys={['0-5']} description="Set rating for selected image(s)" />
                <KeybindItem keys={['↑', '↓', '←', '→']} description="Navigate images in library" />
              </div>
              <div className="pt-2">
                <h3 className="text-lg font-semibold pt-3 pb-2 text-accent">Editor</h3>
                <KeybindItem keys={['Esc']} description="Exit editor / fullscreen" />
                <KeybindItem keys={['Ctrl/Cmd', '+', 'Z']} description="Undo adjustment" />
                <KeybindItem keys={['Ctrl/Cmd', '+', 'Y']} description="Redo adjustment" />
                <KeybindItem keys={['Space']} description="Toggle 200% zoom" />
                <KeybindItem keys={['←', '→']} description="Previous / Next image" />
                <KeybindItem keys={['↑', '↓']} description="Zoom out / Zoom in" />
                <KeybindItem keys={['F']} description="Toggle fullscreen" />
                <KeybindItem keys={['B']} description="Show original (before/after)" />
                <KeybindItem keys={['R']} description="Toggle Crop panel" />
                <KeybindItem keys={['M']} description="Toggle Masks panel" />
                <KeybindItem keys={['I']} description="Toggle Metadata panel" />
                <KeybindItem keys={['W']} description="Toggle Waveform display" />
                <KeybindItem keys={['E']} description="Toggle Export panel" />
              </div>
            </div>
          </div>

          <div className="p-6 bg-surface rounded-xl shadow-md">
            <h2 className="text-xl font-semibold mb-2 text-accent">Information</h2>
            {appSettings?.lastRootPath && (
              <div className="mt-4">
                <h3 className="font-medium text-text-primary">Last Used Folder</h3>
                <p className="text-sm text-text-secondary bg-bg-primary p-3 rounded-md mt-2 font-mono break-all">
                  {appSettings.lastRootPath}
                </p>
              </div>
            )}
          </div>

        </div>
      </div>
    </>
  );
}