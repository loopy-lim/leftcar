# Cloudflare Pages Deployment

This site is deployed to Cloudflare Pages as a static artifact.

## Automated Deployment
Deployment is fully automated via GitHub Actions (`.github/workflows/deploy-pages.yml`).
- Pushes to the `main` branch will automatically deploy to the `leftcar-site` Cloudflare Pages project.
- It requires `CLOUDFLARE_ACCOUNT_ID` and `CLOUDFLARE_API_TOKEN` to be set as GitHub Repository Secrets.

## Manual Deployment
If you need to manually deploy the site, follow these steps:

1. Install dependencies:
   ```bash
   bun install
   ```

2. Build the static assets:
   ```bash
   bun run build
   ```

3. Authenticate with Cloudflare (if not already authenticated):
   ```bash
   npx wrangler login
   ```

4. Deploy the `dist` directory to Cloudflare Pages:
   ```bash
   npx wrangler pages deploy dist --project-name="leftcar-site" --branch="main"
   ```
