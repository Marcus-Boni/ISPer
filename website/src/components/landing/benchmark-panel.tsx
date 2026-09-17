"use client";

import { useState } from "react";
import { useTablist } from "@/lib/use-tablist";

/**
 * Two numbers, one of them measured.
 *
 * The previous version plotted five monthly points on a calendar axis. There is
 * one measurement — `content/docs/referencia/benchmarks.md` — and no time
 * dimension at all, so the shape of the chart was asserting data that does not
 * exist, under a heading that promises the reader can audit it.
 *
 * The form is an emphasis comparison: the measured value carries the accent, the
 * scenario recedes. Provenance rides on each mark rather than living in a table
 * column that scrolls off a phone.
 */

type Mark = {
  label: string;
  /** Lower bound, and for a scenario the upper bound of the stated range. */
  from: number;
  to?: number;
  display: string;
  provenance: string;
  kind: "measured" | "scenario" | "contract";
};

type View = {
  id: "tempo" | "custo";
  tab: string;
  title: string;
  note: string;
  unit: string;
  scaleMax: number;
  marks: [Mark, Mark];
  tableHeading: string;
};

const views: View[] = [
  {
    id: "tempo",
    tab: "Tempo",
    title: "Tempo para transcrever 10,4 s de áudio",
    note: "Uma medição publicada, comparada a um cenário de nuvem. Não é um benchmark de fornecedor.",
    unit: "segundos",
    scaleMax: 4,
    marks: [
      {
        label: "ISPer local",
        from: 0.6,
        display: "0,6 s",
        provenance: "medido · RTX 4050 · Large v3 Turbo q5",
        kind: "measured",
      },
      {
        label: "Serviço em nuvem",
        from: 2.5,
        to: 3.8,
        display: "2,5–3,8 s",
        provenance: "cenário ilustrativo · sem fornecedor atribuído",
        kind: "scenario",
      },
    ],
    tableHeading: "Resultado de referência",
  },
  {
    id: "custo",
    tab: "Custo",
    title: "Tarifa de transcrição após 5 meses",
    note: "Cenário de 600 min/mês a R$ 0,06/min. O ISPer não cobra tarifa de transcrição.",
    unit: "reais acumulados",
    scaleMax: 180,
    marks: [
      {
        label: "ISPer local",
        from: 0,
        display: "R$ 0,00",
        provenance: "contrato do produto · transcrição local",
        kind: "contract",
      },
      {
        label: "Serviço em nuvem",
        from: 180,
        display: "R$ 180,00",
        provenance: "cenário ilustrativo · 3.000 min tarifados",
        kind: "scenario",
      },
    ],
    tableHeading: "Após 5 meses",
  },
];

const provenanceLabel = { measured: "Medido", scenario: "Cenário", contract: "Contrato" } as const;

export function BenchmarkPanel() {
  const [active, setActive] = useState<View["id"]>("tempo");
  const view = views.find((item) => item.id === active) ?? views[0];
  const { list: tabList, tabProps } = useTablist(["tempo", "custo"] as const, active, setActive);

  return (
    <div className="benchmark-panel">
      <div className="chart-head">
        <div>
          <h3>{view.title}</h3>
          <p>{view.note}</p>
        </div>
        <div className="chart-tabs" role="tablist" aria-label="Métrica comparada" ref={tabList}>
          {views.map((item) => (
            <button key={item.id} id={`chart-tab-${item.id}`} type="button" aria-controls="benchmark-figure" {...tabProps(item.id)}>
              {item.tab}
            </button>
          ))}
        </div>
      </div>

      <div id="benchmark-figure" role="tabpanel" aria-labelledby={`chart-tab-${view.id}`} className="measure-list">
        {view.marks.map((mark) => {
          const start = (mark.from / view.scaleMax) * 100;
          const end = ((mark.to ?? mark.from) / view.scaleMax) * 100;
          return (
            <div key={mark.label} className={`measure-row is-${mark.kind}`}>
              <span className="measure-label">{mark.label}</span>
              <span className="measure-track">
                <span className="measure-fill" style={{ width: `${start}%` }} />
                {mark.to ? <span className="measure-range" style={{ left: `${start}%`, width: `${end - start}%` }} /> : null}
              </span>
              <span className="measure-value">{mark.display}</span>
              <span className="measure-source">
                <b>{provenanceLabel[mark.kind]}</b>
                <span>{mark.provenance.replace(/^[^·]+· ?/, "")}</span>
              </span>
            </div>
          );
        })}
        <p className="measure-scale">
          escala 0–{view.scaleMax.toLocaleString("pt-BR")} {view.unit}
        </p>
      </div>

      <table className="benchmark-table">
        <caption>Mesmos dados em tabela, com a natureza de cada número</caption>
        <thead>
          <tr>
            <th scope="col">Opção</th>
            <th scope="col">{view.tableHeading}</th>
            <th scope="col">Natureza do dado</th>
          </tr>
        </thead>
        <tbody>
          {view.marks.map((mark) => (
            <tr key={mark.label}>
              <th scope="row">{mark.label}</th>
              <td>{mark.display}</td>
              <td>{mark.provenance}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
