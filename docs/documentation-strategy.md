# Documentation Strategy

## Principle: README is the source of truth

Every layer's documentation lives in its `README.md`. The documentation website is **generated** from these files — never edited directly.

```
    layers/fabric/README.md          ← source of truth (you edit this)
         │
    scripts/sync-docs.sh             ← converts to MDX, escapes JSX chars
         │
    documentation/src/app/           ← generated pages (do not edit)
    fabric/page.mdx
         │
    npm run build                    ← Next.js static export
         │
    documentation/out/               ← static HTML deployed to GitHub Pages
```

## How it works

### The sync script

`scripts/sync-docs.sh` reads markdown files from two locations and generates MDX pages:

| Source | Generates | Example |
|---|---|---|
| `layers/*/README.md` | `documentation/src/app/*/page.mdx` | `layers/fabric/README.md` → `fabric/page.mdx` |
| `docs/*.md` | `documentation/src/app/*/page.mdx` | `docs/cli.md` → `cli/page.mdx` |
| `docs/ARCHITECTURE.md` | `documentation/src/app/page.mdx` | Homepage |

The script:
1. Reads each markdown file
2. Escapes MDX-incompatible characters (`<5` → `&lt;5`, `{word}` → `&#123;word&#125;`)
3. Adds an `export const metadata` block (title, description)
4. Adds a `{/* AUTO-GENERATED */}` comment
5. Writes the result as a `.mdx` page file

### GitHub Actions

On push to `main` (when docs or layers change):

```
1. Checkout repo
2. Run scripts/sync-docs.sh          ← generate pages from READMEs
3. npm ci                             ← install dependencies
4. npm run build                      ← Next.js static export
5. Deploy to GitHub Pages             ← live at sifrah.github.io/syfrah/
```

### Local development

```bash
just docs-sync     # sync READMEs → MDX pages
just docs          # sync + build
just docs-serve    # start Next.js dev server (localhost:3000)
```

For quick iteration, edit the MDX page directly and use `just docs-serve`. The sync script will overwrite your local changes at the next sync — so remember to **edit the README, not the MDX page** for permanent changes.

## Adding a new page

### New layer

1. Create `layers/{name}/README.md` with the standard template
2. Add the layer to `scripts/sync-docs.sh` (LAYERS, LAYER_TITLES, LAYER_DESCS arrays)
3. Add the page to `documentation/src/components/Navigation.tsx`
4. Run `just docs-sync` to verify

### New cross-cutting doc

1. Create `docs/{name}.md`
2. Add the doc to `scripts/sync-docs.sh` (DOCS, DOC_TITLES, DOC_DESCS arrays)
3. Add the page to `documentation/src/components/Navigation.tsx`
4. Run `just docs-sync` to verify

### Navigation

The sidebar navigation is defined in `documentation/src/components/Navigation.tsx`. It must be updated manually when adding or removing pages. This is intentional — the navigation order and grouping are editorial decisions, not auto-discoverable.

## Updating content

1. Edit the README in the layer (e.g., `layers/overlay/README.md`)
2. Push to `main`
3. GitHub Actions runs sync + build + deploy
4. Site is updated automatically

No need to touch anything in `documentation/`. The sync script handles everything.

## What lives where

| Location | What | Edited by |
|---|---|---|
| `layers/*/README.md` | Layer concept documentation | Developers (source of truth) |
| `docs/*.md` | Cross-cutting documentation | Developers (source of truth) |
| `scripts/sync-docs.sh` | Sync script (README → MDX) | Rarely (only when adding pages) |
| `documentation/src/app/*/page.mdx` | Generated MDX pages | **Nobody** (auto-generated) |
| `documentation/src/components/Navigation.tsx` | Sidebar navigation | When adding/removing pages |
| `documentation/src/components/` | Site framework (layout, code blocks, search) | Rarely |
| `.github/workflows/docs.yml` | CI/CD pipeline | Rarely |

## Generated files

Files in `documentation/src/app/*/page.mdx` are committed to git but are **generated**. They have a comment at the top:

```
{/* AUTO-GENERATED from layers/fabric/README.md — do not edit */}
```

They are committed (not gitignored) so that:
- `just docs-serve` works without running sync first
- PR diffs show documentation changes
- The repo is self-contained

## Technology

| Component | Technology | Why |
|---|---|---|
| Site framework | Next.js 15 + MDX | ZeroFS-based, dark theme, code highlighting, search |
| Styling | Tailwind CSS + typography plugin | Consistent, responsive |
| Code highlighting | Shiki | Syntax highlighting at build time |
| Search | FlexSearch | Client-side, no external service |
| Deployment | GitHub Pages via Actions | Free, automatic |
| Sync | Bash script | Simple, no dependencies |
