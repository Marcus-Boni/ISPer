/**
 * Argumentos do scripts/sync-release.mjs.
 *
 * Mora aqui, e não no próprio script, porque o sync-release roda `main()` ao
 * ser importado — preso lá dentro, o parser não tinha como ser testado. E
 * não ser testado escondeu um bug: `--tag v1.2.3` (com espaço, a forma que o
 * cabeçalho do script documenta e a que o workflow portal-release-sync usa)
 * lia o valor mas não o pulava, então a volta seguinte tratava `v1.2.3` como
 * argumento desconhecido e o script morria. Só `--tag=v1.2.3` funcionava.
 */

const TAG = /^v\d+\.\d+\.\d+/;

export function parseArgs(argv) {
  const args = { check: false, tag: null };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--check") {
      args.check = true;
    } else if (arg === "--tag") {
      const value = argv[i + 1];
      if (value === undefined || value.startsWith("--")) {
        throw new Error("--tag precisa de um valor, como --tag v1.2.3");
      }
      args.tag = value;
      i += 1; // o valor já foi consumido: não é um argumento próprio
    } else if (arg.startsWith("--tag=")) {
      args.tag = arg.slice("--tag=".length);
    } else {
      throw new Error(`argumento desconhecido: ${arg}`);
    }
  }
  if (args.tag !== null && !TAG.test(args.tag)) {
    throw new Error(`--tag precisa parecer uma tag de versão, recebi: ${args.tag}`);
  }
  return args;
}
