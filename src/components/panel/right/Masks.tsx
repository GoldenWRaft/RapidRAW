import React from 'react';
import { Brush, Circle, Eraser, Sparkles, TriangleRight, User } from 'lucide-react';

export enum Mask {
  AiForeground = 'ai-foreground',
  AiSubject = 'ai-subject',
  Brush = 'brush',
  Color = 'color',
  Linear = 'linear',
  Luminance = 'luminance',
  QuickEraser = 'quick-eraser',
  Radial = 'radial',
}

export enum SubMaskMode {
  Additive = 'additive',
  Subtractive = 'subtractive',
}

export enum ToolType {
  AiSeletor = 'ai-selector',
  Brush = 'brush',
  Eraser = 'eraser',
  GenerativeReplace = 'generative-replace',
  SelectSubject = 'select-subject',
}

export interface MaskType {
  disabled: boolean;
  icon: any;
  id?: string;
  name: string;
  type: Mask;
}

export interface SubMask {
  id: string;
  mode: SubMaskMode;
  parameters?: any;
  type: Mask;
  visible: boolean;
}

export const MASK_TYPES: Array<MaskType> = [
  {
    disabled: false,
    icon: Eraser,
    name: 'Quick Erase',
    type: Mask.QuickEraser,
},
  {
    disabled: false,
    icon: Sparkles,
    name: 'Subject',
    type: Mask.AiSubject,
  },
  {
    disabled: false,
    icon: User,
    name: 'Foreground',
    type: Mask.AiForeground,
  },
  {
    disabled: false,
    icon: Brush,
    name: 'Brush',
    type: Mask.Brush,
  },
  {
    disabled: false,
    icon: TriangleRight,
    name: 'Linear',
    type: Mask.Linear,
  },
  {
    disabled: false,
    icon: Circle,
    name: 'Radial',
    type: Mask.Radial,
  },
];
