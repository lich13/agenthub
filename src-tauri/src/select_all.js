(() => {
  const target = document.activeElement;
  if (!target) return;
  const event = new KeyboardEvent("keydown", {
    key: "a", metaKey: true, bubbles: true, cancelable: true,
  });
  if (target.dispatchEvent(event)) document.execCommand("selectAll");
})();
