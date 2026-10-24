// The three small windows the tray opens: delay, log, and the overwrite notice.
// The page never sees transcript text; it only talks to the Rust commands below.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

const app = document.querySelector<HTMLElement>("#app")!;
const close = () => getCurrentWindow().close();

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> = {},
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}

async function delayView() {
  const current = await invoke<number>("get_delay");
  const input = el("input", { type: "number", min: "0", max: "60", step: "0.1", value: String(current) });
  const message = el("p", { className: "error" });
  const save = el("button", { className: "primary", textContent: "保存" });
  save.onclick = async () => {
    const value = Number(input.value);
    if (!Number.isFinite(value) || value < 0 || value > 60) {
      message.textContent = "0〜60 の数で入力してください。";
      return;
    }
    await invoke("set_delay", { seconds: value });
    await close();
  };
  app.replaceChildren(
    el("h1", { textContent: "コピーまでの待機時間" }),
    el("p", { className: "muted", textContent: "新しい文字起こしを検出してから待つ秒数（0〜60 秒、0.1 秒刻み）" }),
    el("div", { className: "row" }, input, "秒"),
    message,
    el("div", { className: "row" }, save, el("button", { textContent: "キャンセル", onclick: close })),
  );
}

async function logView() {
  const text = await invoke<string>("read_log");
  app.replaceChildren(
    el("h1", { textContent: "ログ（本文は記録しません）" }),
    el("pre", { textContent: text || "ログはありません。" }),
    el("div", { className: "row" }, el("button", { textContent: "閉じる", onclick: close })),
  );
}

function noticeView() {
  app.replaceChildren(
    el("h1", { textContent: "クリップボード上書きについて" }),
    el("p", { textContent: "このアプリは新しい文字起こしを検出してから、設定秒数後にクリップボードへコピーします。" }),
    el("p", { textContent: "待機中に別の内容をコピーすると、文字起こしで上書きされる場合があります。必要に応じて自動コピーをOFFにし、手動コピーを使ってください。" }),
    el("p", { textContent: "コピー後に別の内容をコピーした場合、同じ文字起こしを再コピーしません。本文はログに残さず、ネットワーク送信もしません。" }),
    el("p", { className: "muted", textContent: "macOS のユニバーサルクリップボードや Windows のクリップボード履歴・同期が有効だと、コピーした本文が他の機器や履歴に渡る場合があります。このアプリはそれらの設定を変えません。" }),
    el("div", { className: "row" }, el("button", { className: "primary", textContent: "OK", onclick: close })),
  );
}

const views: Record<string, () => unknown> = { "#delay": delayView, "#log": logView, "#notice": noticeView };
(views[location.hash] ?? noticeView)();
