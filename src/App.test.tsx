import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { setDesktopForTests, setSheetsForTests } from "./lib/desktop";
import type { DesktopApi, SheetsApi } from "./lib/desktop";

const sampleCatalog = {
  availableStructs: ["Sensor", "Packet"],
  defaultRoot: "Packet",
};

const sampleResult = {
  rootName: "Packet",
  availableStructs: ["Sensor", "Packet"],
  totalSize: 12,
  warnings: [],
  rows: [
    {
      status: "Initial",
      parameterName: "id",
      systemParameterName: "id",
      dataType: "UINT",
      dataSize: 32,
      inIoBuffer: "Yes",
      ioBufferOffset: 0,
      comments: "编号",
    },
    {
      status: "Initial",
      parameterName: "code",
      systemParameterName: "code",
      dataType: "UINT16",
      dataSize: 16,
      inIoBuffer: "Yes",
      ioBufferOffset: 4,
      comments: "",
    },
  ],
};

function mockDesktop(): DesktopApi {
  return {
    kind: "mock",
    minimizeWindow: vi.fn(async () => {}),
    toggleMaximizeWindow: vi.fn(async () => {}),
    closeWindow: vi.fn(async () => {}),
    pickFiles: vi.fn(async () => []),
    pickFolder: vi.fn(async () => null),
    saveFile: vi.fn(async () => "C:/tmp/Packet.xlsx"),
    revealInFolder: vi.fn(async () => {}),
    openPath: vi.fn(async () => {}),
    appVersion: vi.fn(async () => "0.3.0"),
  };
}

function mockSheets(overrides: Partial<SheetsApi> = {}): SheetsApi {
  return {
    listStructs: vi.fn(async () => sampleCatalog),
    parseSource: vi.fn(async () => sampleResult),
    exportExcel: vi.fn(async () => "C:/tmp/Packet.xlsx"),
    convertTableToXMacro: vi.fn(async () => ({
      sourcePath: "粘贴的 Excel 内容",
      sheetName: "粘贴内容",
      macroName: "T_FIELDS",
      structName: "T_TYPE",
      rowCount: 1,
      code: "#define T_FIELDS(X) \\\n    X(float, a, 0, 4)",
      warnings: [],
      rows: [],
    })),
    saveTextFile: vi.fn(async (path: string) => path),
    readTextFile: vi.fn(async () => "typedef struct { uint32_t id; } Packet;"),
    ...overrides,
  };
}

describe("App", () => {
  it("renders the brand and version", async () => {
    setDesktopForTests(mockDesktop());
    setSheetsForTests(mockSheets());
    render(<App />);
    expect(screen.getByText("StructSheet")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByText(/v0\.3\.0/)).toBeInTheDocument());
  });

  it("parses the sample source and shows preview rows", async () => {
    setDesktopForTests(mockDesktop());
    const sheetsApi = mockSheets();
    setSheetsForTests(sheetsApi);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: /解析结构体/ }));
    await waitFor(() => expect(sheetsApi.parseSource).toHaveBeenCalled());
    expect((await screen.findAllByText("id")).length).toBeGreaterThan(0);
    expect(await screen.findByText("UINT16")).toBeInTheDocument();
  });

  it("shows parse errors from the backend", async () => {
    setDesktopForTests(mockDesktop());
    setSheetsForTests(
      mockSheets({
        listStructs: vi.fn(async () => {
          throw new Error("没有找到完整的结构体定义。");
        }),
      }),
    );
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: /解析结构体/ }));
    expect(await screen.findByText("解析没有完成")).toBeInTheDocument();
    expect(await screen.findByText(/没有找到完整的结构体定义/)).toBeInTheDocument();
  });

  it("exports to an xlsx path chosen in the save dialog", async () => {
    const desktopApi = mockDesktop();
    setDesktopForTests(desktopApi);
    const sheetsApi = mockSheets();
    setSheetsForTests(sheetsApi);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: /解析结构体/ }));
    await screen.findAllByText("id");
    await userEvent.click(screen.getByRole("button", { name: /导出 Excel/ }));
    await waitFor(() => expect(sheetsApi.exportExcel).toHaveBeenCalled());
    expect(desktopApi.saveFile).toHaveBeenCalled();
    expect(await screen.findByText(/已导出/)).toBeInTheDocument();
  });

  it("opens the X-Macro dialog and converts pasted cells", async () => {
    setDesktopForTests(mockDesktop());
    const sheetsApi = mockSheets();
    setSheetsForTests(sheetsApi);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: /表格转代码/ }));
    expect(await screen.findByText("表格转 X-Macro")).toBeInTheDocument();
    const paste = screen.getByPlaceholderText(/先在 Excel 中选中/);
    await userEvent.type(paste, "Bytes\tParameter Name\tType\n0~3\ta\tfloat");
    await userEvent.click(screen.getByRole("button", { name: /解析并生成代码/ }));
    await waitFor(() => expect(sheetsApi.convertTableToXMacro).toHaveBeenCalled());
    expect(await screen.findByDisplayValue(/T_FIELDS/)).toBeInTheDocument();
  });
});
