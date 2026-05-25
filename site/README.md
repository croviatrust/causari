# causari.dev — landing site

Static, zero-build, deployed to **Cloudflare Pages** from `site/` of this repo.

## Local preview

```powershell
# Any static server. Pick one:
npx serve .
# or
python -m http.server 8080
```

Then visit `http://localhost:8080`.

## Deploy on Cloudflare Pages

1. Cloudflare Dashboard → **Workers & Pages** → **Create** → **Pages** → **Connect to Git**
2. Select `croviatrust/causari`
3. Build settings:
   - **Framework preset**: None
   - **Build command**: *(leave empty)*
   - **Build output directory**: `site`
4. Deploy. Cloudflare assigns a `*.pages.dev` URL immediately.
5. **Custom domains** → add `causari.dev` and `www.causari.dev`. Cloudflare creates the DNS records automatically since the domain is on the same account.

## Files

- `index.html` — single-page landing
- `styles.css` — design system (CSS variables, dark default, light toggle)
- `app.js` — terminal typer, copy buttons, MCP tabs, theme toggle, GitHub star fetch
- `_headers` — security headers + caching policy
- `_redirects` — vanity URLs (`/github`, `/license`, ...)
- `assets/` — logo SVGs (mirrored from repo root)
- `robots.txt`, `sitemap.xml` — SEO

## Updating logos

The SVGs in `site/assets/` are mirrored from the repo root `assets/`. Whenever
you change the master logo, refresh the copies:

```powershell
Copy-Item assets\*.svg site\assets\ -Force
```
