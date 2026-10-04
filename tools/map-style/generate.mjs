// Стили карты для MapLibre (задача 3.9): слои Protomaps, подписи по-русски, всё —
// со своего сервера. Адрес сервера подставляет deploy.sh вместо {{BASE}}.
//   cd tools/map-style && npm ci && npm run generate
import { writeFileSync } from "node:fs";
import { layers, namedFlavor } from "@protomaps/basemaps";

for (const flavor of ["light", "dark"]) {
  const style = {
    version: 8,
    name: `Staya ${flavor}`,
    glyphs: "{{BASE}}/map/fonts/{fontstack}/{range}.pbf",
    sprite: `{{BASE}}/map/sprites/${flavor}`,
    sources: {
      protomaps: {
        type: "vector",
        url: "{{BASE}}/tiles/region.json",
        attribution:
          '<a href="https://protomaps.com">Protomaps</a> © <a href="https://openstreetmap.org/copyright">OpenStreetMap</a>',
      },
    },
    layers: layers("protomaps", namedFlavor(flavor), { lang: "ru" }),
  };
  const out = new URL(`../../deploy/map/style-${flavor}.json`, import.meta.url);
  writeFileSync(out, JSON.stringify(style) + "\n");
  console.log(`${out.pathname}: ${style.layers.length} layers`);
}
