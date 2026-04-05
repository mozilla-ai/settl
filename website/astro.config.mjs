import { defineConfig } from "astro/config";
import tailwind from "@astrojs/tailwind";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://brake-labs.github.io",
  base: "/settl",
  integrations: [tailwind(), sitemap({ changefreq: "weekly" })],
});
