// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

import tailwindcss from "@tailwindcss/vite";

// https://astro.build/config
export default defineConfig({
  integrations: [
    starlight({
      title: "Hopp Documentation",
      social: [{ icon: "github", label: "GitHub", href: "https://github.com/gethopp/hopp" }],
      logo: {
        light: "./src/assets/logo-light.png",
        dark: "./src/assets/logo-dark.png",
        replacesTitle: true,
      },
      customCss: ["./src/styles/global.css"],
      sidebar: [
        {
          label: "Quick Start",
          items: [
            { label: "What is Hopp?", slug: "quick-start/what-is-hopp" },
            {
              label: "Local Development",
              items: [
                { label: "Supported Platforms", slug: "quick-start/local-development/supported-platforms" },
                { label: "Prerequisites", slug: "quick-start/local-development/prerequisites" },
                { label: "Repository Structure", slug: "quick-start/local-development/repository-structure" },
                { label: "Development Workflow", slug: "quick-start/local-development/development-workflow" },
                { label: "FAQ", slug: "quick-start/local-development/faq" },
              ],
            },
          ],
        },
        {
          label: "Open Source",
          items: [
            { label: "Contribute", slug: "open-source/contribute" },
            { label: "Self-Hosting & Publishing", slug: "open-source/self-hosting" },
          ],
        },
        {
          label: "Features",
          items: [
            { label: "Terminologies", slug: "features/terminologies" },
            { label: "Screen Sharing", slug: "features/screen-sharing" },
            { label: "Rooms", slug: "features/rooms" },
            { label: "Remote Control", slug: "features/remote-control" },
            { label: "Drawing (beta)", slug: "features/drawing" },
          ],
        },
        {
          label: "Integrations",
          items: [{ label: "Slack", slug: "integrations/slack" }],
        },
        {
          label: "Community",
          items: [{ label: "FAQ", slug: "faq" }],
        },
      ],
      components: {
        Hero: "./src/components/Hero.astro",
      },
    }),
  ],

  vite: {
    plugins: [tailwindcss()],
  },
});
