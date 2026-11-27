import { Minus, Square, X } from "lucide-react";
import { desktop } from "../lib/desktop";

/**
 * Windows 风格窗口控制按钮（无系统按钮的隐藏标题栏窗口）。
 * 点击穿透到桌面适配层，浏览器环境为空实现。
 */
export function WindowControls() {
  return (
    <div role="group" className="window-controls" aria-label="窗口控制" data-no-window-drag>
      <button
        type="button"
        aria-label="最小化"
        title="最小化"
        onClick={() => void desktop.minimizeWindow()}
      >
        <Minus size={10} strokeWidth={1.6} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="最大化或还原"
        title="最大化或还原"
        onClick={() => void desktop.toggleMaximizeWindow()}
      >
        <Square size={10} strokeWidth={1.6} aria-hidden="true" />
      </button>
      <button
        type="button"
        aria-label="关闭"
        title="关闭"
        className="close"
        onClick={() => void desktop.closeWindow()}
      >
        <X size={10} strokeWidth={1.6} aria-hidden="true" />
      </button>
    </div>
  );
}
