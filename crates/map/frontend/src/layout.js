// Layered layout for one level of boxes and arrows. Pure: no DOM, no canvas.

export const BOX_WIDTH = 240
export const BOX_HEIGHT = 120
const GAP_X = 48
const GAP_Y = 72
const MARGIN = 40
// A layer wider than this wraps into more rows, so 50 unconnected boxes stay a readable grid.
const MAX_PER_ROW = 5

/** Longest-path layer per box id. An arrow that would close a cycle is ignored. */
export function assignLayers(ids, arrows) {
  const outgoing = new Map(ids.map((id) => [id, []]))
  for (const { from, to } of arrows) outgoing.get(from)?.push(to)
  const state = new Map() // id -> "open" while on the DFS stack, "done" after
  const forward = []
  const visit = (id) => {
    state.set(id, "open")
    for (const to of outgoing.get(id)) {
      if (state.get(to) === "open" || !outgoing.has(to)) continue
      forward.push([id, to])
      if (!state.has(to)) visit(to)
    }
    state.set(id, "done")
  }
  for (const id of ids) if (!state.has(id)) visit(id)

  const layer = new Map(ids.map((id) => [id, 0]))
  // Relaxing |ids| times reaches the fixed point of a longest path in a DAG.
  for (let pass = 0; pass < ids.length; pass++) {
    let changed = false
    for (const [from, to] of forward) {
      if (layer.get(to) < layer.get(from) + 1) {
        layer.set(to, layer.get(from) + 1)
        changed = true
      }
    }
    if (!changed) break
  }
  return layer
}

/** Rows of ids, top to bottom, ordered to keep connected boxes near each other. */
export function orderRows(ids, arrows, layer) {
  const depth = Math.max(0, ...layer.values())
  const columns = new Map()
  const layers = Array.from({ length: depth + 1 }, () => [])
  for (const id of [...ids].sort()) layers[layer.get(id)].push(id)
  const neighbours = new Map(ids.map((id) => [id, []]))
  for (const { from, to } of arrows) {
    neighbours.get(from)?.push(to)
    neighbours.get(to)?.push(from)
  }
  const place = (row) => row.forEach((id, index) => columns.set(id, index))
  layers.forEach(place)
  const barycenter = (id) => {
    const placed = neighbours.get(id).filter((other) => columns.has(other))
    return placed.length
      ? placed.reduce((sum, other) => sum + columns.get(other), 0) / placed.length
      : columns.get(id)
  }
  for (let sweep = 0; sweep < 4; sweep++) {
    for (const row of layers) {
      row.sort((a, b) => barycenter(a) - barycenter(b) || (a < b ? -1 : 1))
      place(row)
    }
  }
  return layers.flatMap((row) => {
    const chunks = []
    for (let start = 0; start < row.length; start += MAX_PER_ROW) {
      chunks.push(row.slice(start, start + MAX_PER_ROW))
    }
    return chunks
  })
}

/** `{ positions: Map<id, {x, y, width, height}>, width, height }`, rows centred. */
export function layout(boxes, arrows) {
  const ids = boxes.map((box) => box.id)
  const rows = orderRows(ids, arrows, assignLayers(ids, arrows))
  const widest = Math.max(1, ...rows.map((row) => row.length))
  const sheetWidth = widest * BOX_WIDTH + (widest - 1) * GAP_X
  const positions = new Map()
  rows.forEach((row, rowIndex) => {
    const rowWidth = row.length * BOX_WIDTH + (row.length - 1) * GAP_X
    const left = MARGIN + (sheetWidth - rowWidth) / 2
    row.forEach((id, index) => {
      positions.set(id, {
        x: left + index * (BOX_WIDTH + GAP_X),
        y: MARGIN + rowIndex * (BOX_HEIGHT + GAP_Y),
        width: BOX_WIDTH,
        height: BOX_HEIGHT,
      })
    })
  })
  return {
    positions,
    width: sheetWidth + 2 * MARGIN,
    height: rows.length * (BOX_HEIGHT + GAP_Y) - GAP_Y + 2 * MARGIN,
  }
}

/** Where the line between two box centres leaves `from` and enters `to`: [x1, y1, x2, y2]. */
export function arrowPoints(from, to) {
  const centre = (box) => ({ x: box.x + box.width / 2, y: box.y + box.height / 2 })
  const a = centre(from)
  const b = centre(to)
  const edge = (box, c, towards) => {
    const dx = towards.x - c.x
    const dy = towards.y - c.y
    const scale = Math.min(
      dx === 0 ? Infinity : box.width / 2 / Math.abs(dx),
      dy === 0 ? Infinity : box.height / 2 / Math.abs(dy),
    )
    return [c.x + dx * scale, c.y + dy * scale]
  }
  return [...edge(from, a, b), ...edge(to, b, a)]
}

/** Stroke width in pixels: thicker for more edges, never thinner than 1 or thicker than 7. */
export function strokeWidth(count, maxCount) {
  return 1 + (6 * Math.log1p(count)) / Math.log1p(Math.max(maxCount, 1))
}
