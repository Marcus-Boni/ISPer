import type { ComponentType } from "react";
import BuscaSemantica from "../../content/docs/busca-semantica/configuracao.md";
import BuscaPrivacidade from "../../content/docs/busca-semantica/privacidade.md";
import Compilar from "../../content/docs/desenvolvimento/compilar.md";
import Diarizacao from "../../content/docs/identificacao-de-falantes/diarizacao.md";
import Providers from "../../content/docs/inteligencia-e-resumos/providers.md";
import Ollama from "../../content/docs/inteligencia-e-resumos/ollama.md";
import Instalacao from "../../content/docs/primeiros-passos/instalacao.md";
import VisaoGeral from "../../content/docs/primeiros-passos/visao-geral.md";
import Modelos from "../../content/docs/primeiros-passos/modelos-whisper.md";
import PrimeiroDitado from "../../content/docs/primeiros-passos/primeiro-ditado.md";
import Reunioes from "../../content/docs/reunioes-e-sistema/captura-audio-lgpd.md";
import Teams from "../../content/docs/reunioes-e-sistema/teams.md";
import Privacidade from "../../content/docs/reunioes-e-sistema/privacidade.md";
import Benchmarks from "../../content/docs/referencia/benchmarks.md";
import Hardware from "../../content/docs/referencia/hardware.md";
import Releases from "../../content/docs/referencia/releases.md";
import Audio from "../../content/docs/solucao-de-problemas/audio.md";
import Atalhos from "../../content/docs/solucao-de-problemas/atalhos.md";
import Cuda from "../../content/docs/solucao-de-problemas/cuda.md";
import Faq from "../../content/docs/solucao-de-problemas/faq.md";

export const docComponents: Record<string, ComponentType> = {
  "busca-semantica/configuracao": BuscaSemantica,
  "busca-semantica/privacidade": BuscaPrivacidade,
  "desenvolvimento/compilar": Compilar,
  "identificacao-de-falantes/diarizacao": Diarizacao,
  "inteligencia-e-resumos/providers": Providers,
  "inteligencia-e-resumos/ollama": Ollama,
  "primeiros-passos/instalacao": Instalacao,
  "primeiros-passos/visao-geral": VisaoGeral,
  "primeiros-passos/modelos-whisper": Modelos,
  "primeiros-passos/primeiro-ditado": PrimeiroDitado,
  "reunioes-e-sistema/captura-audio-lgpd": Reunioes,
  "reunioes-e-sistema/teams": Teams,
  "reunioes-e-sistema/privacidade": Privacidade,
  "referencia/benchmarks": Benchmarks,
  "referencia/hardware": Hardware,
  "referencia/releases": Releases,
  "solucao-de-problemas/audio": Audio,
  "solucao-de-problemas/atalhos": Atalhos,
  "solucao-de-problemas/cuda": Cuda,
  "solucao-de-problemas/faq": Faq,
};
