/** 报告当前页面是否运行在 MyGo 桌面窗口内。 */
export function isMyGo(): boolean {
  return typeof globalThis !== "undefined" && Boolean((globalThis as { mygo?: unknown }).mygo);
}

/** 报告浏览器（开发服务器直开）环境。 */
export function isBrowserHost(): boolean {
  return !isMyGo();
}
