# ISPer Website

Portal estático oficial do ISPer, isolado do app desktop. O site usa Next.js App Router com `output: "export"` e publica o conteúdo final em `out/`, pronto para Cloudflare Pages sem runtime de servidor.

## Desenvolvimento

```bash
corepack enable
corepack prepare pnpm@12.3.4 --activate
pnpm install --frozen-lockfile
pnpm dev
```

O desenvolvimento acontece dentro desta pasta. Não crie workspace Node na raiz do monorepo e não importe código runtime de `apps/isper-app`.

## Validação local

```bash
node scripts/validate-content.mjs
pnpm exec eslint .
pnpm exec tsc --noEmit
node scripts/prepare-content.mjs
pnpm exec next build
pnpm exec pagefind --site out
node scripts/verify-export.mjs
node scripts/check-links.mjs
node scripts/check-budgets.mjs
```

`next build` já gera o export estático em `out/` por causa de `output: "export"`. Os scripts de verificação usam APIs nativas do Node para manter a infraestrutura simples e reproduzível.

## Deploy

O workflow `.github/workflows/website.yml` roda em PRs, em pushes para `main`, em releases publicadas e manualmente. O deploy de produção acontece apenas em push para `main` e somente quando estes segredos/variáveis existem no GitHub Actions:

- `CLOUDFLARE_API_TOKEN`
- `CLOUDFLARE_ACCOUNT_ID`
- `CLOUDFLARE_PAGES_PROJECT_NAME` opcional, padrão `isper`

O job de deploy usa o mesmo artefato `out/` validado no job de build. Se as credenciais Cloudflare estiverem ausentes, o workflow registra o skip no resumo e mantém os checks locais verdes.

## Cloudflare Pages

Configuração esperada:

- Build command: `pnpm exec next build && pnpm exec pagefind --site out`
- Diretório de saída: `out`
- Node: `22`
- Package manager: `pnpm@12.3.4`

Os arquivos `public/_headers` e `public/_redirects` são copiados para `out/` pelo export e aplicados pelo Cloudflare Pages.

## Orçamentos

`scripts/check-budgets.mjs` mede a transferência inicial de cada rota representativa e falha quando JavaScript, CSS, HTML ou o total ultrapassam os limites definidos. Lighthouse fica configurado em `lighthouserc.cjs`; sua execução depende de disponibilizar `@lhci/cli` no ambiente de qualidade.
