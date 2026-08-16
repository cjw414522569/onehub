import type { AiChartSpec } from "../../shared/tauri/commands";

interface ChartViewProps {
  spec: AiChartSpec;
}

const W = 640;
const H = 260;
const PAD = 36;

export function ChartView({ spec }: ChartViewProps) {
  const labels = spec.labels || [];
  const series = spec.series || [];
  if (!spec.chart_type || spec.chart_type === "none" || series.length === 0) {
    return <p style={{ color: "#6b7280", fontSize: 12 }}>无法生成图表：{spec.reason || "无数据"}。</p>;
  }
  if (spec.chart_type === "pie") {
    const total = series[0].values.reduce((sum, value) => sum + Math.max(0, value), 0) || 1;
    let angle = -90;
    const arcs = series[0].values.map((value, index) => {
      const start = angle;
      const sweep = (Math.max(0, value) / total) * 360;
      angle += sweep;
      const largeArc = sweep > 180 ? 1 : 0;
      const cx = 160;
      const cy = 140;
      const r = 90;
      const x1 = cx + r * Math.cos((start * Math.PI) / 180);
      const y1 = cy + r * Math.sin((start * Math.PI) / 180);
      const x2 = cx + r * Math.cos(((start + sweep) * Math.PI) / 180);
      const y2 = cy + r * Math.sin(((start + sweep) * Math.PI) / 180);
      return (
        <path
          key={index}
          d={`M ${cx} ${cy} L ${x1} ${y1} A ${r} ${r} 0 ${largeArc} 1 ${x2} ${y2} Z`}
          fill={`hsl(${(index * 137) % 360} 70% 55%)`}
          stroke="#ffffff"
          strokeWidth={1}
        />
      );
    });
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
        <svg width={320} height={280} viewBox="0 0 320 280">
          {arcs}
        </svg>
        <ul style={{ listStyle: "none", margin: 0, padding: 0, fontSize: 12 }}>
          {labels.map((label, index) => (
            <li key={index} style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 4 }}>
              <span style={{ width: 10, height: 10, background: `hsl(${(index * 137) % 360} 70% 55%)`, borderRadius: 2, display: "inline-block" }} />
              {label}: {series[0].values[index]}
            </li>
          ))}
        </ul>
      </div>
    );
  }

  // line / bar
  const allValues = series.flatMap((s) => s.values);
  const max = Math.max(...allValues, 1);
  const min = Math.min(...allValues, 0);
  const range = max - min || 1;
  const n = Math.max(labels.length, 1);
  const stepX = (W - PAD * 2) / n;
  const barGroup = stepX * 0.7;
  const seriesCount = Math.max(series.length, 1);
  const barWidth = barGroup / seriesCount;

  const y = (value: number) => H - PAD - ((value - min) / range) * (H - PAD * 2);
  const x = (index: number) => PAD + index * stepX + stepX / 2;


  return (
    <svg width="100%" height={H} viewBox={`0 0 ${W} ${H}`} style={{ background: "#ffffff", border: "1px solid #e5e7eb", borderRadius: 4 }}>
      {Array.from({ length: 5 }).map((_, index) => {
        const value = min + (range * index) / 4;
        const yy = y(value);
        return (
          <g key={index}>
            <line x1={PAD} y1={yy} x2={W - PAD} y2={yy} stroke="#eef0f3" strokeWidth={1} />
            <text x={PAD - 6} y={yy + 4} textAnchor="end" fontSize={10} fill="#6b7280">
              {value.toFixed(0)}
            </text>
          </g>
        );
      })}
      {spec.chart_type === "bar"
        ? series.map((s, seriesIndex) =>
            s.values.map((value, index) => (
              <rect
                key={`${seriesIndex}-${index}`}
                x={PAD + index * stepX + (barGroup / seriesCount) * seriesIndex}
                y={y(Math.max(value, 0))}
                width={Math.max(barWidth - 2, 1)}
                height={Math.abs(y(Math.max(value, 0)) - y(0))}
                fill={`hsl(${((seriesIndex + 1) * 137) % 360} 70% 55%)`}
              />
            )),
          )
        : series.map((s, seriesIndex) => (
            <polyline
              key={seriesIndex}
              points={s.values
                .map((value, index) => `${x(index)},${y(value)}`)
                .join(" ")}
              fill="none"
              stroke={`hsl(${((seriesIndex + 1) * 137) % 360} 70% 55%)`}
              strokeWidth={2}
            />
          ))}
      {labels.map((label, index) => (
        <text key={index} x={x(index)} y={H - PAD + 14} textAnchor="middle" fontSize={10} fill="#6b7280">
          {label.length > 10 ? `${label.slice(0, 9)}…` : label}
        </text>
      ))}
    </svg>
  );
}