/**
 * UI 状态归一化：把 Go 侧或测试 mock 抛出的错误转成中文提示串。
 * mygo-runtime 的 CallError 的 message 即 Go error 文本。
 */
export function errorMessage(reason: unknown): string {
  if (reason instanceof Error) {
    return reason.message || String(reason);
  }
  return String(reason);
}
