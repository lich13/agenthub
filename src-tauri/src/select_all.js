(() => {
  const target = document.activeElement;
  if (!target) return;
  if (target.tagName === "TEXTAREA" ||
      (target.tagName === "INPUT" && !["checkbox", "radio", "button", "submit", "reset", "image", "file", "range", "color"].includes(target.type))) {
    target.select();
    return;
  }
  if (target.isContentEditable) {
    document.execCommand("selectAll");
    return;
  }
  if (target.closest('dialog,[role="dialog"],[role="alertdialog"],[role="menu"]')) return;
  const event = new KeyboardEvent("keydown", {
    key: "a", metaKey: true, bubbles: true, cancelable: true,
  });
  target.dispatchEvent(event);
})();
