// Shared standard DOM workload: compiled by Perry or executed by Electron.
(function (): void {
    const document: any = (globalThis as any).document;
    const increment: any = document.getElementById('increment');
    const update: any = document.getElementById('update');
    const reorder: any = document.getElementById('reorder');
    const rebuild: any = document.getElementById('rebuild');
    const status: any = document.getElementById('status');
    const benchmarkState: any = document.getElementById('benchmark-state');
    const records: any = document.getElementById('records');
    if (increment === null || update === null || reorder === null ||
        rebuild === null || status === null || benchmarkState === null || records === null) {
        throw new Error('The task dashboard is missing its required elements');
    }

    // Each initialization owns its document, mutable state and node arrays.
    let count = 0;
    let revision = 0;
    let generation = 0;
    let ascending = true;
    const rows: any[] = [];
    const cells: any[] = [];

    // Direct ordinary callbacks use fresh per-document closure allocations.
    // Keep the loops inline: helper-function variables use a singleton-cache
    // compiler path that this trusted package deliberately does not admit.
    increment.addEventListener('click', function (): void {
        count = count + 1;
        status.textContent = 'Perry clicks: ' + count;
    });
    update.addEventListener('click', function (): void {
        revision = revision + 1;
        for (let i = 0; i < 500; i = i + 1) {
            cells[i * 5].textContent = 'Task ' + i;
            cells[i * 5 + 1].textContent = 'Title ' + i + ' revision ' + revision;
            cells[i * 5 + 2].textContent = 'Owner ' + (i % 8);
            cells[i * 5 + 3].textContent = 'Priority ' + (i % 4);
            cells[i * 5 + 4].textContent = 'Points ' + ((i * 17 + revision * 13) % 101);
        }
        let order = 'asc';
        if (!ascending) order = 'desc';
        benchmarkState.textContent = 'update:' + revision + ':' + generation + ':' + order;
        console.log('PERRY_DOM_DONE update ' + revision + ' ' + generation + ' ' + order);
    });
    reorder.addEventListener('click', function (): void {
        ascending = !ascending;
        for (let position = 0; position < 500; position = position + 1) {
            let i = position;
            if (!ascending) i = 499 - position;
            records.appendChild(rows[i]);
        }
        let order = 'asc';
        if (!ascending) order = 'desc';
        benchmarkState.textContent = 'reorder:' + revision + ':' + generation + ':' + order;
        console.log('PERRY_DOM_DONE reorder ' + revision + ' ' + generation + ' ' + order);
    });
    rebuild.addEventListener('click', function (): void {
        generation = generation + 1;
        for (let i = 0; i < 500; i = i + 1) records.removeChild(rows[i]);
        for (let i = 0; i < 500; i = i + 1) {
            const row: any = document.createElement('article');
            rows[i] = row;
            for (let field = 0; field < 5; field = field + 1) {
                const cell: any = document.createElement('p');
                cells[i * 5 + field] = cell;
                row.appendChild(cell);
            }
            cells[i * 5].textContent = 'Task ' + i;
            cells[i * 5 + 1].textContent = 'Title ' + i + ' revision ' + revision;
            cells[i * 5 + 2].textContent = 'Owner ' + (i % 8);
            cells[i * 5 + 3].textContent = 'Priority ' + (i % 4);
            cells[i * 5 + 4].textContent = 'Points ' + ((i * 17 + revision * 13) % 101);
        }
        for (let position = 0; position < 500; position = position + 1) {
            let i = position;
            if (!ascending) i = 499 - position;
            records.appendChild(rows[i]);
        }
        let order = 'asc';
        if (!ascending) order = 'desc';
        benchmarkState.textContent = 'rebuild:' + revision + ':' + generation + ':' + order;
        console.log('PERRY_DOM_DONE rebuild ' + revision + ' ' + generation + ' ' + order);
    });
    status.textContent = 'Perry clicks: 0';
    for (let i = 0; i < 500; i = i + 1) {
        const row: any = document.createElement('article');
        rows[i] = row;
        for (let field = 0; field < 5; field = field + 1) {
            const cell: any = document.createElement('p');
            cells[i * 5 + field] = cell;
            row.appendChild(cell);
        }
        cells[i * 5].textContent = 'Task ' + i;
        cells[i * 5 + 1].textContent = 'Title ' + i + ' revision ' + revision;
        cells[i * 5 + 2].textContent = 'Owner ' + (i % 8);
        cells[i * 5 + 3].textContent = 'Priority ' + (i % 4);
        cells[i * 5 + 4].textContent = 'Points ' + ((i * 17 + revision * 13) % 101);
        records.appendChild(row);
    }
    benchmarkState.textContent = 'initial:0:0:asc';
    console.log('PERRY_DOM_DONE initial 0 0 asc');
})();
