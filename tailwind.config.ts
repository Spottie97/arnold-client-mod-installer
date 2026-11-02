import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        bg: "var(--bg)",
        "grad-1": "var(--grad-1)",
        "grad-2": "var(--grad-2)",
        panel: "var(--panel)",
        text: "var(--text)",
        muted: "var(--muted)",
        primary: "var(--primary)",
        "primary-hover": "var(--primary-hover)",
        accent: "var(--accent)",
        "accent-deep": "var(--accent-deep)",
        gold: "var(--gold)",
        success: "var(--success)",
        error: "var(--error)",
        glow: "var(--glow)",
        arnold: "var(--arnold)",
      },
      fontFamily: {
        heading: "var(--font-heading)",
        body: "var(--font-body)",
      },
      boxShadow: {
        card: "0 0 0 1px rgba(157, 92, 255, 0.18)",
        selected: "0 0 0 2px var(--accent), 0 12px 40px rgba(88, 44, 160, 0.35)",
      },
    },
  },
  plugins: [],
} satisfies Config;
