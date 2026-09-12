// 生成 qingbird-git 应用图标：蓝色渐变圆角方块 + 白色飞鸟剪影
// 用法: node scripts/gen-icons.mjs  （在项目根目录执行）
import zlib from "node:zlib";
import fs from "node:fs";
import path from "node:path";

// ---------- PNG 编码 ----------
const CRC_TABLE = (() => {
  const t = new Int32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c;
  }
  return t;
})();
function crc32(buf) {
  let c = -1;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}
function encodePNG(w, h, rgba) {
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  const raw = Buffer.alloc((w * 4 + 1) * h);
  for (let y = 0; y < h; y++) {
    raw[y * (w * 4 + 1)] = 0;
    rgba.copy(raw, y * (w * 4 + 1) + 1, y * w * 4, (y + 1) * w * 4);
  }
  return Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

// ---------- 图形绘制 ----------
// 多边形（点在多边形内判断）
function inPoly(px, py, pts) {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i];
    const [xj, yj] = pts[j];
    if (yi > py !== yj > py && px < ((xj - xi) * (py - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}
function inCircle(px, py, cx, cy, r) {
  const dx = px - cx, dy = py - cy;
  return dx * dx + dy * dy <= r * r;
}
function inRoundRect(px, py, size, radius) {
  if (px < 0 || py < 0 || px > size || py > size) return false;
  const r = radius;
  const cx = Math.min(Math.max(px, r), size - r);
  const cy = Math.min(Math.max(py, r), size - r);
  return (px - cx) ** 2 + (py - cy) ** 2 <= r * r || (px >= r && px <= size - r) || (py >= r && py <= size - r);
}
// 精确圆角矩形：内缩判断
function roundRectSDF(px, py, size, radius) {
  const half = size / 2;
  const qx = Math.abs(px - half) - (half - radius);
  const qy = Math.abs(py - half) - (half - radius);
  const ax = Math.max(qx, 0), ay = Math.max(qy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(qx, qy), 0) - radius;
}

// 图标主体：白色圆角几何风格 "git" 字样，坐标按 0..100 设计稿（x-height 34..62，基线 62）
function birdCoverage(px, py) {
  const segDist = (px, py, x1, y1, x2, y2) => {
    const dx = x2 - x1, dy = y2 - y1;
    const t = Math.min(1, Math.max(0, ((px - x1) * dx + (py - y1) * dy) / (dx * dx + dy * dy)));
    return Math.hypot(px - (x1 + t * dx), py - (y1 + t * dy));
  };
  const ring = (cx, cy, ro, ri) => inCircle(px, py, cx, cy, ro) && !inCircle(px, py, cx, cy, ri);
  const cap = (x1, y1, x2, y2, r) => segDist(px, py, x1, y1, x2, y2) <= r;

  // g：圆环碗形 + 右侧竖笔下探 + 左勾
  const g =
    ring(27, 48, 14.5, 6.5) ||
    cap(41.5, 48, 41.5, 76, 5) ||
    cap(41.5, 76, 33, 79, 5);
  // i：短竖笔 + 上方圆点
  const i = cap(60, 40, 60, 62, 5) || inCircle(px, py, 60, 29, 5.5);
  // t：竖笔（顶部到基线略下）+ 底部右勾 + 横杠
  const t =
    cap(80, 32, 80, 70, 5) ||
    cap(80, 70, 88, 73, 5) ||
    cap(70, 42, 90, 42, 5);
  return g || i || t ? 1 : 0;
}

function render(size, ss = 4) {
  const S = size * ss;
  const rgba = Buffer.alloc(size * size * 4);
  // 渐变色：青 → 蓝（青鸟）
  const c1 = [56, 189, 248]; // #38BDF8
  const c2 = [37, 99, 235]; // #2563EB
  const radius = size * 0.2237; // squircle-ish
  const scale = 100 / S;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      // 超采样
      let aBg = 0, aBird = 0;
      let rBg = 0, gBg = 0, bBg = 0;
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const px = (x * ss + sx + 0.5) * scale;
          const py = (y * ss + sy + 0.5) * scale;
          // 背景：圆角矩形 + 对角渐变
          const d = roundRectSDF(px, py, 100, 22.37);
          const bgCov = Math.min(1, Math.max(0, 0.5 - d));
          if (bgCov > 0) {
            const t = Math.min(1, Math.max(0, (px + py) / 200));
            rBg += (c1[0] + (c2[0] - c1[0]) * t) * bgCov;
            gBg += (c1[1] + (c2[1] - c1[1]) * t) * bgCov;
            bBg += (c1[2] + (c2[2] - c1[2]) * t) * bgCov;
            aBg += bgCov;
          }
          if (birdCoverage(px, py)) aBird += 1;
        }
      }
      const n = ss * ss;
      aBg /= n; rBg /= n || 1; gBg /= n || 1; bBg /= n || 1;
      aBird /= n;
      // 合成：白色飞鸟覆盖在渐变上
      let r = rBg, g = gBg, b = bBg, a = aBg;
      const birdA = aBird * aBg;
      r = r * (1 - birdA) + 255 * birdA;
      g = g * (1 - birdA) + 255 * birdA;
      b = b * (1 - birdA) + 255 * birdA;
      const i = (y * size + x) * 4;
      rgba[i] = Math.round(r);
      rgba[i + 1] = Math.round(g);
      rgba[i + 2] = Math.round(b);
      rgba[i + 3] = Math.round(a * 255);
    }
  }
  return encodePNG(size, size, rgba);
}

function makeIco(png256) {
  const entry = Buffer.alloc(16);
  entry[0] = 0; // 256
  entry[1] = 0;
  entry[2] = 0;
  entry[3] = 0;
  entry.writeUInt16LE(1, 4);
  entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(png256.length, 8);
  entry.writeUInt32LE(22, 12);
  const dir = Buffer.alloc(6);
  dir.writeUInt16LE(0, 0);
  dir.writeUInt16LE(1, 2);
  dir.writeUInt16LE(1, 4);
  return Buffer.concat([dir, entry, png256]);
}

const iconsDir = path.resolve("src-tauri/icons");
fs.mkdirSync(iconsDir, { recursive: true });
const targets = [30, 32, 44, 50, 71, 89, 107, 128, 142, 150, 256, 284, 310, 512];
for (const size of targets) {
  const png = render(size);
  fs.writeFileSync(path.join(iconsDir, `gen-${size}.png`), png);
  console.log(`gen-${size}.png done`);
}
const png256 = fs.readFileSync(path.join(iconsDir, "gen-256.png"));
fs.writeFileSync(path.join(iconsDir, "icon.ico"), makeIco(png256));
// 复制到标准命名
const rename = {
  "gen-32.png": "32x32.png",
  "gen-128.png": "128x128.png",
  "gen-256.png": "128x128@2x.png",
  "gen-512.png": "icon.png",
  "gen-30.png": "Square30x30Logo.png",
  "gen-44.png": "Square44x44Logo.png",
  "gen-71.png": "Square71x71Logo.png",
  "gen-89.png": "Square89x89Logo.png",
  "gen-107.png": "Square107x107Logo.png",
  "gen-142.png": "Square142x142Logo.png",
  "gen-150.png": "Square150x150Logo.png",
  "gen-284.png": "Square284x284Logo.png",
  "gen-310.png": "Square310x310Logo.png",
  "gen-50.png": "StoreLogo.png",
  "gen-256.png": "tray.png",
};
for (const [from, to] of Object.entries(rename)) {
  fs.copyFileSync(path.join(iconsDir, from), path.join(iconsDir, to));
}
for (const size of targets) fs.rmSync(path.join(iconsDir, `gen-${size}.png`));
console.log("icons generated:", iconsDir);
