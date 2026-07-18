// Generates Sanctum's app icons: a dark rounded tile with the sage shield-and-
// check mark (logo "Option 3"). Drawn procedurally in a 48-unit design space,
// 4x supersampled for smooth edges. No third-party deps.
//   node tools/gen_icons.mjs
import { deflateSync } from "node:zlib";
import { writeFileSync, mkdirSync } from "node:fs";

const TILE = [12, 14, 13]; // #0c0e0d
const SAGE = [143, 188, 165]; // #8fbca5
const D = 48; // design units
const R = 10.5; // tile corner radius

// Shield outline (curved sides sampled into a polygon).
const SHIELD = [
  [24, 8], [38, 13], [38, 25], [37, 30], [34, 35], [30, 39], [24, 42],
  [18, 39], [14, 35], [11, 30], [10, 25], [10, 13],
];
const CHECK = [[18, 24], [22.5, 28.5], [30.5, 19.5]];
const CHECK_HW = 1.95;

function inPoly(px, py, poly) {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const [xi, yi] = poly[i];
    const [xj, yj] = poly[j];
    if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}
function distSeg(px, py, ax, ay, bx, by) {
  const dx = bx - ax, dy = by - ay, l2 = dx * dx + dy * dy;
  let t = l2 ? ((px - ax) * dx + (py - ay) * dy) / l2 : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy));
}
function onCheck(px, py) {
  for (let i = 0; i < CHECK.length - 1; i++) {
    if (distSeg(px, py, CHECK[i][0], CHECK[i][1], CHECK[i + 1][0], CHECK[i + 1][1]) <= CHECK_HW) return true;
  }
  return false;
}
function inRoundRect(px, py) {
  const rx = Math.max(R - px, px - (D - R), 0);
  const ry = Math.max(R - py, py - (D - R), 0);
  return rx * rx + ry * ry <= R * R;
}
function sample(px, py) {
  if (!inRoundRect(px, py)) return [0, 0, 0, 0];
  if (inPoly(px, py, SHIELD)) return onCheck(px, py) ? [...TILE, 255] : [...SAGE, 255];
  return [...TILE, 255];
}

function crc32(buf) {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}
function chunk(type, data) {
  const t = Buffer.from(type, "ascii");
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([t, data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function png(size) {
  const SS = 4;
  const raw = Buffer.alloc((size * 4 + 1) * size);
  let o = 0;
  for (let y = 0; y < size; y++) {
    raw[o++] = 0; // filter: none
    for (let x = 0; x < size; x++) {
      let r = 0, g = 0, b = 0, a = 0;
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const px = ((x + (sx + 0.5) / SS) / size) * D;
          const py = ((y + (sy + 0.5) / SS) / size) * D;
          const c = sample(px, py);
          r += c[0] * c[3];
          g += c[1] * c[3];
          b += c[2] * c[3];
          a += c[3];
        }
      }
      raw[o++] = a > 0 ? Math.round(r / a) : 0;
      raw[o++] = a > 0 ? Math.round(g / a) : 0;
      raw[o++] = a > 0 ? Math.round(b / a) : 0;
      raw[o++] = Math.round(a / (SS * SS));
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;
  ihdr[9] = 6; // RGBA
  const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

function ico(pngBuf, size) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(1, 4);
  const entry = Buffer.alloc(16);
  entry[0] = size >= 256 ? 0 : size;
  entry[1] = size >= 256 ? 0 : size;
  entry.writeUInt16LE(1, 4);
  entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(pngBuf.length, 8);
  entry.writeUInt32LE(22, 12);
  return Buffer.concat([header, entry, pngBuf]);
}

const dir = new URL("../ui/src-tauri/icons/", import.meta.url);
mkdirSync(dir, { recursive: true });
const write = (name, buf) => writeFileSync(new URL(name, dir), buf);

write("32x32.png", png(32));
write("128x128.png", png(128));
write("128x128@2x.png", png(256));
write("icon.png", png(512));
write("icon.ico", ico(png(256), 256));
console.log("Wrote Sanctum shield-and-check icons to ui/src-tauri/icons/");
