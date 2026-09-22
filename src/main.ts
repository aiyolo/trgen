declare global {
  interface Window {
    __structSheetBootTimer?: number;
  }
}

export {};

const application = import("./bootstrap");

requestAnimationFrame(async () => {
  const { mountApplication } = await application;
  if (window.__structSheetBootTimer !== undefined) {
    window.clearInterval(window.__structSheetBootTimer);
  }
  mountApplication();
});
