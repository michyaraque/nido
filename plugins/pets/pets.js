/* Mascotas: on a phone the section tabs scroll sideways, so bring the open one into view.
   Everything else works without this script. */
(() => {
  const init = (doc) => {
    const on = doc.querySelector(".pt-tabs a.on");
    const bar = on && on.parentElement;
    if (bar && bar.scrollWidth > bar.clientWidth) bar.scrollLeft = on.offsetLeft - (bar.clientWidth - on.offsetWidth) / 2;
  };
  (window.nidoPlugins ||= {}).pets = init;
  init(document);
})();
