import { useState, useEffect, useRef } from 'react';
import { save, open } from '@tauri-apps/plugin-dialog';
import { invoke } from '@tauri-apps/api/core';
import { Save, CheckCircle, XCircle, Loader, Ban } from 'lucide-react';
import Switch from '../../ui/Switch';
import { Adjustments } from '../../../utils/adjustments';
import {
  ExportSettings,
  FileFormat,
  FILE_FORMATS,
  FILENAME_VARIABLES,
  Status,
  ExportState,
  FileFormats,
} from './ExportImportProperties';
import { Invokes, SelectedImage } from '../../ui/AppProperties';

interface ExportPanelProps {
  adjustments: Adjustments;
  exportState: ExportState;
  multiSelectedPaths: Array<string>;
  selectedImage: SelectedImage;
  setExportState(state: any): void;
}

interface SectionProps {
  children: any;
  title: string;
}

function Section({ title, children }: SectionProps) {
  return (
    <div>
      <h3 className="text-sm font-semibold text-text-primary mb-3 border-b border-surface pb-2">{title}</h3>
      <div className="space-y-4">{children}</div>
    </div>
  );
}

export default function ExportPanel({
  adjustments,
  exportState,
  multiSelectedPaths,
  selectedImage,
  setExportState,
}: ExportPanelProps) {
  const [fileFormat, setFileFormat] = useState<string>('jpeg');
  const [jpegQuality, setJpegQuality] = useState<number>(90);
  const [enableResize, setEnableResize] = useState<boolean>(false);
  const [resizeMode, setResizeMode] = useState<string>('longEdge');
  const [resizeValue, setResizeValue] = useState<number>(2048);
  const [dontEnlarge, setDontEnlarge] = useState<boolean>(true);
  const [keepMetadata, setKeepMetadata] = useState<boolean>(true);
  const [stripGps, setStripGps] = useState<boolean>(true);
  const [filenameTemplate, setFilenameTemplate] = useState<string>('{original_filename}_edited');
  const filenameInputRef = useRef<HTMLInputElement>(null);

  const { status, progress, errorMessage } = exportState;
  const isExporting = status === Status.Exporting;

  const isEditorContext = !!selectedImage;
  const pathsToExport = isEditorContext
    ? multiSelectedPaths.length > 0
      ? multiSelectedPaths
      : selectedImage
      ? [selectedImage.path]
      : []
    : multiSelectedPaths;
  const numImages = pathsToExport.length;
  const isBatchMode = numImages > 1;

  useEffect(() => {
    if (!isExporting) {
      setExportState({ status: Status.Idle, progress: { current: 0, total: 0 }, errorMessage: '' });
    }
  }, [selectedImage, multiSelectedPaths, isExporting, setExportState]);

  const handleVariableClick = (variable: string) => {
    if (!filenameInputRef.current) {
      return;
    }

    const input: HTMLInputElement = filenameInputRef.current;
    const start = Number(input.selectionStart);
    const end = Number(input.selectionEnd);
    const currentValue = input.value;

    const newValue = currentValue.substring(0, start) + variable + currentValue.substring(end);
    setFilenameTemplate(newValue);

    setTimeout(() => {
      input.focus();
      const newCursorPos = start + variable.length;
      input.setSelectionRange(newCursorPos, newCursorPos);
    }, 0);
  };

  const handleExport = async () => {
    if (numImages === 0 || isExporting) {
      return;
    }

    setExportState({ status: Status.Exporting, progress: { current: 0, total: numImages }, errorMessage: '' });

    let finalFilenameTemplate = filenameTemplate;
    if (isBatchMode && !filenameTemplate.includes('{sequence}') && !filenameTemplate.includes('{original_filename}')) {
      finalFilenameTemplate = `${filenameTemplate}_{sequence}`;
      setFilenameTemplate(finalFilenameTemplate);
    }

    const exportSettings: ExportSettings = {
      filenameTemplate: finalFilenameTemplate,
      jpegQuality: jpegQuality,
      keepMetadata,
      resize: enableResize ? { mode: resizeMode, value: resizeValue, dontEnlarge } : null,
      stripGps,
    };

    try {
      if (isBatchMode || !isEditorContext) {
        const outputFolder = await open({ title: `Select Folder to Export ${numImages} Image(s)`, directory: true });
        if (outputFolder) {
          await invoke(Invokes.BatchExportImages, {
            exportSettings,
            outputFolder,
            outputFormat: FILE_FORMATS.find((f: FileFormat) => f.id === fileFormat)?.extensions[0],
            paths: pathsToExport,
          });
        } else {
          setExportState((prev: ExportState) => ({ ...prev, status: Status.Idle }));
        }
      } else {
        const selectedFormat: any = FILE_FORMATS.find((f) => f.id === fileFormat);
        const originalFilename = selectedImage.path.split(/[\\/]/).pop();
        const name = originalFilename ? originalFilename?.split('.') : '';
        const filePath = await save({
          title: 'Save Edited Image',
          defaultPath: `${name}_edited.${selectedFormat.extensions[0]}`,
          filters: FILE_FORMATS.map((f: FileFormat) => ({ name: f.name, extensions: f.extensions })),
        });
        if (filePath) {
          await invoke(Invokes.ExportImage, {
            exportSettings,
            jsAdjustments: adjustments,
            originalPath: selectedImage.path,
            outputPath: filePath,
          });
        } else {
          setExportState((prev: ExportState) => ({ ...prev, status: Status.Idle }));
        }
      }
    } catch (error) {
      console.error('Failed to start export:', error);
      setExportState({
        errorMessage: typeof error === 'string' ? error : 'Failed to start export.',
        progress,
        status: Status.Error,
      });
    }
  };

  const handleCancel = async () => {
    try {
      await invoke(Invokes.CancelExport);
    } catch (error) {
      console.error('Failed to send cancel request:', error);
    }
  };

  const canExport = numImages > 0;

  return (
    <div className="flex flex-col h-full">
      <div className="p-4 flex justify-between items-center flex-shrink-0 border-b border-surface">
        <h2 className="text-xl font-bold text-primary text-shadow-shiny">
          Export {numImages > 1 ? `(${numImages})` : ''}
        </h2>
      </div>
      <div className="flex-grow overflow-y-auto p-4 text-text-secondary space-y-6">
        {canExport ? (
          <>
            <Section title="File Settings">
              <div className="grid grid-cols-3 gap-2">
                {FILE_FORMATS.map((format: FileFormat) => (
                  <button
                    className={`px-2 py-1.5 text-sm rounded-md transition-colors ${
                      fileFormat === format.id ? 'bg-surface text-white' : 'bg-surface hover:bg-card-active'
                    } disabled:opacity-50`}
                    disabled={isExporting}
                    key={format.id}
                    onClick={() => setFileFormat(format.id)}
                  >
                    {format.name}
                  </button>
                ))}
              </div>
              {fileFormat === FileFormats.Jpeg && (
                <div className="flex items-center gap-2">
                  <label className="text-sm w-20">Quality</label>
                  <input
                    className="w-full h-1 bg-surface rounded-lg appearance-none cursor-pointer accent-accent"
                    disabled={isExporting}
                    max="100"
                    min="1"
                    onChange={(e: React.ChangeEvent<HTMLInputElement>) => setJpegQuality(parseInt(e?.target?.value))}
                    type="range"
                    value={jpegQuality}
                  />
                  <span className="text-sm font-mono w-12 text-right">{jpegQuality}</span>
                </div>
              )}
            </Section>

            {isBatchMode && (
              <Section title="File Naming">
                <input
                  className="w-full bg-bg-primary border border-surface rounded-md p-2 text-sm text-text-primary focus:ring-accent focus:border-accent"
                  disabled={isExporting}
                  onChange={(e: React.ChangeEvent<HTMLInputElement>) => setFilenameTemplate(e.target.value)}
                  ref={filenameInputRef}
                  type="text"
                  value={filenameTemplate}
                />
                <div className="flex flex-wrap gap-2 mt-2">
                  {FILENAME_VARIABLES.map((variable: string) => (
                    <button
                      className="px-2 py-1 bg-surface text-text-secondary text-xs rounded-md hover:bg-card-active transition-colors disabled:opacity-50"
                      disabled={isExporting}
                      key={variable}
                      onClick={() => handleVariableClick(variable)}
                    >
                      {variable}
                    </button>
                  ))}
                </div>
              </Section>
            )}

            <Section title="Image Sizing">
              <Switch label="Resize to Fit" checked={enableResize} onChange={setEnableResize} disabled={isExporting} />
              {enableResize && (
                <div className="space-y-4 pl-2 border-l-2 border-surface">
                  <div className="flex items-center gap-2">
                    <select
                      className="w-full bg-bg-primary border border-surface rounded-md p-2 text-sm text-text-primary focus:ring-accent focus:border-accent"
                      disabled={isExporting}
                      onChange={(e: React.ChangeEvent<HTMLSelectElement>) => setResizeMode(e?.target?.value)}
                      value={resizeMode}
                    >
                      <option value="longEdge">Long Edge</option>
                      <option value="width">Width</option>
                      <option value="height">Height</option>
                    </select>
                    <input
                      className="w-24 bg-bg-primary text-center rounded-md p-2 border border-surface focus:border-accent focus:ring-accent"
                      disabled={isExporting}
                      min="1"
                      onChange={(e: React.ChangeEvent<HTMLInputElement>) => setResizeValue(parseInt(e?.target?.value))}
                      type="number"
                      value={resizeValue}
                    />
                    <span className="text-sm">pixels</span>
                  </div>
                  <Switch
                    checked={dontEnlarge}
                    disabled={isExporting}
                    label="Don't Enlarge"
                    onChange={setDontEnlarge}
                  />
                </div>
              )}
            </Section>

            <Section title="Metadata">
              <Switch
                checked={keepMetadata}
                disabled={isExporting}
                label="Keep Original Metadata"
                onChange={setKeepMetadata}
              />
              {keepMetadata && (
                <div className="pl-2 border-l-2 border-surface">
                  <Switch label="Remove GPS Data" checked={stripGps} onChange={setStripGps} disabled={isExporting} />
                </div>
              )}
            </Section>
          </>
        ) : (
          <p className="text-center text-text-tertiary mt-4">No image selected for export.</p>
        )}
      </div>

      <div className="p-4 border-t border-surface flex-shrink-0 space-y-3">
        {isExporting ? (
          <button
            className="w-full flex items-center justify-center gap-2 px-4 py-2.5 bg-red-600/80 text-white font-bold rounded-lg hover:bg-red-600 transition-all"
            onClick={handleCancel}
          >
            <Ban size={18} />
            Cancel Export
          </button>
        ) : (
          <button
            className="w-full flex items-center justify-center gap-2 px-4 py-2.5 bg-surface text-white font-bold rounded-lg hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed transition-all"
            disabled={!canExport || isExporting}
            onClick={handleExport}
          >
            <Save size={18} />
            Export {numImages > 1 ? `${numImages} Images` : 'Image'}
          </button>
        )}

        {status === Status.Exporting && (
          <div className="flex items-center gap-2 text-accent mt-3 text-sm justify-center">
            <Loader size={16} className="animate-spin" />
            <span>{`Exporting... (${progress.current}/${progress.total})`}</span>
          </div>
        )}
        {status === Status.Success && (
          <div className="flex items-center gap-2 text-green-400 mt-3 text-sm justify-center">
            <CheckCircle size={16} />
            <span>Export successful!</span>
          </div>
        )}
        {status === Status.Error && (
          <div className="flex items-center gap-2 text-red-400 mt-3 text-sm justify-center text-center">
            <XCircle size={16} />
            <span>{errorMessage}</span>
          </div>
        )}
        {status === Status.Cancelled && (
          <div className="flex items-center gap-2 text-yellow-400 mt-3 text-sm justify-center">
            <Ban size={16} />
            <span>Export cancelled.</span>
          </div>
        )}
      </div>
    </div>
  );
}
