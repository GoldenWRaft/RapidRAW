import Slider from '../ui/Slider';
import { Adjustments, BasicAdjustment } from '../../utils/adjustments';

interface BasicAdjustmentsProps {
  adjustments: Adjustments;
  setAdjustments(adjustments: Partial<Adjustments>): any;
}

export default function BasicAdjustments({ adjustments, setAdjustments }: BasicAdjustmentsProps) {
  const handleAdjustmentChange = (key: BasicAdjustment, value: any) => {
    const numericValue = parseFloat(value);
    setAdjustments((prev: Partial<Adjustments>) => ({ ...prev, [key]: numericValue }));
  };

  return (
    <div>
      <Slider
        label="Exposure"
        max={5}
        min={-5}
        onChange={(e: any) => handleAdjustmentChange(BasicAdjustment.Exposure, e.target.value)}
        step={0.01}
        value={adjustments.exposure}
      />
      <Slider
        label="Contrast"
        max={100}
        min={-100}
        onChange={(e: any) => handleAdjustmentChange(BasicAdjustment.Contrast, e.target.value)}
        step={1}
        value={adjustments.contrast}
      />
      <Slider
        label="Highlights"
        max={100}
        min={-100}
        onChange={(e: any) => handleAdjustmentChange(BasicAdjustment.Highlights, e.target.value)}
        step={1}
        value={adjustments.highlights}
      />
      <Slider
        label="Shadows"
        max={100}
        min={-100}
        onChange={(e: any) => handleAdjustmentChange(BasicAdjustment.Shadows, e.target.value)}
        step={1}
        value={adjustments.shadows}
      />
      <Slider
        label="Whites"
        max={100}
        min={-100}
        onChange={(e: any) => handleAdjustmentChange(BasicAdjustment.Whites, e.target.value)}
        step={1}
        value={adjustments.whites}
      />
      <Slider
        label="Blacks"
        max={100}
        min={-100}
        onChange={(e: any) => handleAdjustmentChange(BasicAdjustment.Blacks, e.target.value)}
        step={1}
        value={adjustments.blacks}
      />
    </div>
  );
}
