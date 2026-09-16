"use client";

import { useState } from "react";
import AreaChart, { Area } from "@/components/charts/area-chart";
import { Grid } from "@/components/charts/grid";
import { ChartTooltip } from "@/components/charts/tooltip/chart-tooltip";

const timeData = [
  { date: new Date("2026-01-01"), local: .62, cloud: 2.8 },
  { date: new Date("2026-02-01"), local: .65, cloud: 3.2 },
  { date: new Date("2026-03-01"), local: .59, cloud: 2.5 },
  { date: new Date("2026-04-01"), local: .61, cloud: 3.8 },
  { date: new Date("2026-05-01"), local: .6, cloud: 3.1 },
];

const costData = [
  { date: new Date("2026-01-01"), local: 0, cloud: 36 },
  { date: new Date("2026-02-01"), local: 0, cloud: 72 },
  { date: new Date("2026-03-01"), local: 0, cloud: 108 },
  { date: new Date("2026-04-01"), local: 0, cloud: 144 },
  { date: new Date("2026-05-01"), local: 0, cloud: 180 },
];

export function BenchmarkChart() {
  const [tab, setTab] = useState<"tempo" | "custo">("tempo");
  const data = tab === "tempo" ? timeData : costData;
  const unit = tab === "tempo" ? "s" : "R$";
  return (
    <div className="benchmark-panel">
      <div className="chart-head">
        <div><h3>{tab === "tempo" ? "Tempo para 10,4 s de áudio" : "Tarifa acumulada de transcrição"}</h3><p>{tab === "tempo" ? "Medição local publicada no README; nuvem é cenário ilustrativo." : "Cenário ilustrativo de 600 min/mês a R$ 0,06/min."}</p></div>
        <div className="chart-tabs" role="tablist" aria-label="Métrica do gráfico"><button id="chart-tab-time" type="button" role="tab" aria-selected={tab === "tempo"} aria-controls="benchmark-chart-panel" onClick={() => setTab("tempo")}>Tempo</button><button id="chart-tab-cost" type="button" role="tab" aria-selected={tab === "custo"} aria-controls="benchmark-chart-panel" onClick={() => setTab("custo")}>Custo</button></div>
      </div>
      <div id="benchmark-chart-panel" className="chart-canvas" role="tabpanel" aria-labelledby={tab === "tempo" ? "chart-tab-time" : "chart-tab-cost"} aria-label={tab === "tempo" ? "Gráfico comparativo de tempo" : "Gráfico comparativo de custo acumulado"}>
        <AreaChart data={data} aspectRatio="2 / 1" animationDuration={700} margin={{ top: 24, right: 24, bottom: 30, left: 24 }}>
          <Grid horizontal vertical={false} stroke="var(--line-2)" strokeOpacity={.45} />
          <Area dataKey="cloud" stroke="var(--muted)" fill="var(--muted)" fillOpacity={.12} />
          <Area dataKey="local" stroke="var(--accent)" fill="var(--accent)" fillOpacity={.28} showMarkers />
          <ChartTooltip showDatePill={false} rows={(point) => [
            { label: "ISPer local", value: unit === "s" ? `${Number(point.local).toFixed(2)} s` : `R$ ${Number(point.local).toFixed(2)}`, color: "var(--accent)" },
            { label: "Cenário nuvem", value: unit === "s" ? `${Number(point.cloud).toFixed(2)} s` : `R$ ${Number(point.cloud).toFixed(2)}`, color: "var(--muted)" },
          ]} />
        </AreaChart>
      </div>
      <table className="benchmark-table"><caption>Resumo acessível dos dados do gráfico</caption><thead><tr><th>Opção</th><th>{tab === "tempo" ? "Resultado de referência" : "Após 5 meses"}</th><th>Natureza do dado</th></tr></thead><tbody><tr><th>ISPer local</th><td>{tab === "tempo" ? "0,6 s em RTX 4050" : "R$ 0,00 em tarifa de API"}</td><td>{tab === "tempo" ? "Medição publicada" : "Contrato do produto"}</td></tr><tr><th>Serviço em nuvem</th><td>{tab === "tempo" ? "2,5–3,8 s" : "R$ 180,00"}</td><td>Cenário ilustrativo, não benchmark de fornecedor</td></tr></tbody></table>
    </div>
  );
}
