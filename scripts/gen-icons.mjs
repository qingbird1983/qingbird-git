// 生成 qingbird-git 应用图标：朱砂印章圆角方（同兄弟项目 qingbird-md-rust 的
// 「青」印底色 #B3483A）+ 暖白 "git" 字标 + 内描边 + 整印倾斜（字不斜）。
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
  // 印章设计参数（与 TitleBar 的 seal-btn / 兄弟项目 gen-app-icon.py 同源）：
  // 朱砂底 #B3483A、暖白字 #F9F3EA、圆角比例 0.214、整印倾斜（CSS rotate(-3deg) 同视觉）
  const SEAL = 86; // 印章边长（100 设计稿内留透明边距：倾斜 + 平台圆角安全区）
  const RADIUS = SEAL * 0.214;
  const BG = [179, 72, 58]; // --zhu #B3483A
  const FG = [249, 243, 234]; // 暖白
  const STROKE_A = 0.18; // 内描边 rgba(255,255,255,.18)
  const STROKE_INSET = SEAL * 0.008;
  const STROKE_W = SEAL * 0.005;
  const TILT = (4 * Math.PI) / 180; // 视觉逆时针（CSS rotate(-4deg) 同向）
  const LETTER_K = 0.7; // 字标缩放：100 稿字形 → 印章内占比
  // 字形包围盒中心（100 稿）：x 12.5..95 / y 23.5..84
  const GC = [53.75, 53.75];
  const cosT = Math.cos(TILT);
  const sinT = Math.sin(TILT);
  const scale = 100 / S;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      // 超采样
      let aBg = 0, aStr = 0, aLet = 0;
      let rBg = 0, gBg = 0, bBg = 0;
      for (let sy = 0; sy < ss; sy++) {
        for (let sx = 0; sx < ss; sx++) {
          const px = (x * ss + sx + 0.5) * scale;
          const py = (y * ss + sy + 0.5) * scale;
          // 世界点 → 印章局部系（逆倾斜回正，视觉逆时针 = CSS rotate(-4deg) 同向），
          // 印章 0..SEAL 居中于 100 稿
          const dx = px - 50;
          const dy = py - 50;
          const lx = 43 + dx * cosT - dy * sinT;
          const ly = 43 + dx * sinT + dy * cosT;
          const d = roundRectSDF(lx, ly, SEAL, RADIUS);
          if (d <= 0) {
            aBg += 1;
            rBg += BG[0];
            gBg += BG[1];
            bBg += BG[2];
            // 内描边：贴印章内缘的一圈白纱（对应 CSS inset box-shadow）
            if (d <= -STROKE_INSET && d >= -(STROKE_INSET + STROKE_W)) aStr += 1;
            // 字标正置（底斜字不斜）：按未旋转坐标采样字形，缩放后对中
            const gx = GC[0] + (px - 50) / LETTER_K;
            const gy = GC[1] + (py - 50) / LETTER_K;
            if (birdCoverage(gx, gy)) aLet += 1;
          }
        }
      }
      const n = ss * ss;
      aBg /= n; aStr /= n; aLet /= n;
      rBg /= n || 1; gBg /= n || 1; bBg /= n || 1;
      // 合成：朱砂底 → 内描边白纱 → 暖白字标
      let r = rBg, g = gBg, b = bBg;
      const strMix = aStr * STROKE_A;
      r = r * (1 - strMix) + 255 * strMix;
      g = g * (1 - strMix) + 255 * strMix;
      b = b * (1 - strMix) + 255 * strMix;
      r = r * (1 - aLet) + FG[0] * aLet;
      g = g * (1 - aLet) + FG[1] * aLet;
      b = b * (1 - aLet) + FG[2] * aLet;
      const i = (y * size + x) * 4;
      rgba[i] = Math.round(r);
      rgba[i + 1] = Math.round(g);
      rgba[i + 2] = Math.round(b);
      rgba[i + 3] = Math.round(aBg * 255);
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
