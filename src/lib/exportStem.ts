/**
 * 导出文件名主干规范化：与旧版 StructSheet 行为一致（TR2 系列重命名，
 * 其余取结构体名）。
 */
export function exportStem(rootName: string): string {
  const upper = rootName.trim().toUpperCase();
  if (upper === "TR2_DD_IN" || upper === "TR2_SW_DD_IN") return "TR2_SW_DD_IN";
  if (upper === "TR2_DD_OUT" || upper === "TR2_SW_DD_OUT") return "TR2_SW_DD_OUT";
  return rootName.trim();
}
