// The live preview of an NML page: the page follows the editor through the preview
// server's events.
(() => {
	const page = document.currentScript.dataset.page;
	const events = new EventSource(`/__nml/events?page=${encodeURIComponent(page)}`);
	const main = () => document.querySelector("main.nml-main");

	// The new content of the page: swapped in place, where the reader is.
	events.addEventListener("main", (event) => {
		const current = main();
		if (!current) {
			location.reload();
			return;
		}
		const x = window.scrollX;
		const y = window.scrollY;
		current.innerHTML = event.data;
		window.scrollTo(x, y);
	});

	// What is around the content changed: the title, the stylesheets.
	events.addEventListener("reload", () => location.reload());

	// The editor's cursor, as an offset in the page's source: to the last block that
	// starts at or before it.
	let shown = null;
	events.addEventListener("scroll", (event) => {
		const offset = Number(event.data);
		let target = null;
		for (const block of main()?.querySelectorAll(":scope > [data-nml-at]") ?? []) {
			if (Number(block.dataset.nmlAt) > offset) break;
			target = block;
		}
		if (!target || target === shown) return;
		shown = target;
		const box = target.getBoundingClientRect();
		// Already in view: left alone.
		if (box.top >= 0 && box.bottom <= window.innerHeight) return;
		target.scrollIntoView({ behavior: "smooth", block: "center" });
	});
})();
