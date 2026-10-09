// Н2-05 · проверка PPG-движка скриптом (синус-мок и контроль брака).
// Запуск из корня репо: node self-host/miniapp/test/ppg-test.mjs
//
// Источник математики — сама страница (маркеры PpgEngine-START/END).
// Критерий наряда: «На синус-моке 60 bpm RR восстанавливается в допуске
// 1000±50 мс; брак ловится громко». Проверка на записи с эталонного
// пульсометра (расхождение среднего RR ≤ 5%) — шаг владельца на
// устройстве при деплое (заявлено в PR/README).
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const m = html.match(/\/\/ PpgEngine-START[\s\S]*?\/\/ PpgEngine-END/);
if (!m) throw new Error("PpgEngine не найден в index.html");
const engine = new Function(m[0] + "\nreturn { ppgExtractRR, ppgQualityRefusal };")();
const { ppgExtractRR, ppgQualityRefusal } = engine;

let fails = 0;
function ok(cond, label) {
  if (!cond) { fails += 1; console.error("FAIL " + label); }
}

// Мок камеры: отсчёты через ~33 мс (30 fps), пульс p уд/мин синусоидой
// на базовом уровне 128, амплитуда A, гауссов шум σ, джиттер кадра ±J мс.
function mockRecording(bpm, seconds, opts) {
  const o = Object.assign({ fps: 30, amp: 12.0, noise: 0.8, jitter: 1.0, seed: 42 }, opts || {});
  let s = o.seed;
  const rnd = () => { s = (s * 1103515245 + 12345) % 2147483648; return s / 2147483648; };
  const samples = [];
  let t = 0;
  const dt = 1000 / o.fps;
  while (t < seconds * 1000) {
    const tj = t + (rnd() * 2 - 1) * o.jitter;
    const pulse = Math.sin(2 * Math.PI * (bpm / 60) * (tj / 1000));
    const g = 128 + o.amp * pulse + (rnd() * 2 - 1) * o.noise;
    samples.push({ t: Math.round(tj * 100) / 100, v: g });
    t += dt;
  }
  return samples;
}

function mean(a) { return a.reduce((x, y) => x + y, 0) / a.length; }

// 1. КРИТЕРИЙ НАРЯДА: синус-мок 60 bpm → RR 1000±50 мс.
for (const [bpm, seconds] of [[60, 25], [60, 20]]) {
  const ex = ppgExtractRR(mockRecording(bpm, seconds));
  ok(ex.rr.length >= 8, `мок ${bpm} bpm/${seconds}s: RR восстановлены (${ex.rr.length})`);
  const m = mean(ex.rr);
  ok(Math.abs(m - 1000) <= 50, `мок 60 bpm: средний RR ${m.toFixed(1)} мс в допуске 1000±50`);
  ok(ppgQualityRefusal(ex) === "", `мок 60 bpm: сигнал признан годным`);
}

// 2. Нормальный сердечный диапазон: 60–100 уд/мин. ВАЖНО (Шаг 0):
// 4.5–7 вдох/мин пейсерских темпов — частота ДЫХАНИЯ, а не пульса;
// PPG восстанавливает сердечный RR (полоса 300–2000 мс = 30–200
// уд/мин), пик ~0.1 Гц от дыхания появится в СПЕКТРЕ RR-ряда (Н2-06),
// а не в сырых интервалах.
for (const bpm of [60, 75, 100]) {
  const ex = ppgExtractRR(mockRecording(bpm, 25));
  const m = ex.rr.length ? mean(ex.rr) : -1;
  const expect = 60000 / bpm;
  ok(Math.abs(m - expect) <= 50, `мок ${bpm} уд/мин: средний RR ${m.toFixed(1)} ≈ ${expect.toFixed(0)}`);
}

// 3. Шумный, но годный сигнал (слабая амплитуда, шум, джиттер кадров).
{
  const ex = ppgExtractRR(mockRecording(60, 25, { amp: 6.0, noise: 1.5, jitter: 4.0 }));
  const m = mean(ex.rr);
  ok(Math.abs(m - 1000) <= 50, `шумный мок: средний RR ${m.toFixed(1)} в допуске`);
}

// 4. Брак ловится ГРОМКО: чистый шум без пульса.
{
  const ex = ppgExtractRR(mockRecording(60, 25, { amp: 0.0, noise: 6.0, seed: 7 }));
  ok(ppgQualityRefusal(ex) !== "", "шум без пульса: громкий отказ");
  ok(ex.rr.length === 0 || ex.epochsRejected > 0, "шум без пульса: эпохи отбракованы");
}
// 5. Брак: слишком короткая запись (< 2 эпох).
{
  const ex = ppgExtractRR(mockRecording(60, 8));
  ok(ppgQualityRefusal(ex).indexOf("короче двух эпох") >= 0, "короткая запись: громкий отказ");
}
// 6. Брак: амплитуда ниже шумового пола (палец не на линзе).
{
  const ex = ppgExtractRR(mockRecording(60, 25, { amp: 0.4, noise: 2.0, seed: 11 }));
  ok(ppgQualityRefusal(ex) !== "", "сигнал ниже шумового пола: громкий отказ");
}

// 7. Результат несёт ТОЛЬКО RR и метки качества — сырых кадров нет.
{
  const ex = ppgExtractRR(mockRecording(60, 20));
  const keys = Object.keys(ex).sort().join(",");
  ok(keys === "epochsRejected,epochsTotal,reason,rr", "форма результата: только RR и метки качества, без кадров");
}

if (fails > 0) { console.error(`${fails} провал(ов)`); process.exit(1); }
console.log("PPG-TEST: ALL GREEN — синус-мок 60 bpm в допуске 1000±50 мс, сердечный диапазон 60–100 уд/мин восстанавливается, брак ловится громко, кадров в результате нет");
