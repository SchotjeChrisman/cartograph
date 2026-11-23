import assert from "node:assert/strict"
import { test } from "node:test"
import { arrowPoints, assignLayers, layout, strokeWidth } from "../src/layout.js"

const box = (id) => ({ id })
const arrow = (from, to, count = 1) => ({ from, to, count })

test("a chain gets one layer per step and the callers sit above", () => {
  const layers = assignLayers(["a", "b", "c"], [arrow("a", "b"), arrow("b", "c"), arrow("a", "c")])

  assert.deepEqual([layers.get("a"), layers.get("b"), layers.get("c")], [0, 1, 2])
})

test("a cycle does not hang and still layers every box", () => {
  const layers = assignLayers(["a", "b"], [arrow("a", "b"), arrow("b", "a")])

  assert.equal(layers.size, 2)
  assert.equal(Math.abs(layers.get("a") - layers.get("b")), 1)
})

test("boxes never overlap and wide layers wrap", () => {
  const boxes = Array.from({ length: 12 }, (_, i) => box(`b${i}`))

  const { positions, width, height } = layout(boxes, [])

  const placed = [...positions.values()]
  assert.equal(placed.length, 12)
  for (const [i, p] of placed.entries()) {
    for (const q of placed.slice(i + 1)) {
      const apart = p.x + p.width <= q.x || q.x + q.width <= p.x || p.y + p.height <= q.y || q.y + q.height <= p.y
      assert.ok(apart, `${JSON.stringify(p)} overlaps ${JSON.stringify(q)}`)
    }
    assert.ok(p.x + p.width <= width && p.y + p.height <= height)
  }
  assert.ok(new Set(placed.map((p) => p.y)).size >= 3, "12 boxes need at least 3 rows")
})

test("arrows run between box edges, not centres", () => {
  const from = { x: 0, y: 0, width: 100, height: 50 }
  const to = { x: 0, y: 200, width: 100, height: 50 }

  assert.deepEqual(arrowPoints(from, to), [50, 50, 50, 200])
})

test("stroke width grows with the edge count within 1 to 7", () => {
  assert.ok(strokeWidth(1, 100) < strokeWidth(50, 100))
  assert.ok(strokeWidth(100, 100) <= 7)
  assert.ok(strokeWidth(1, 1) >= 1)
})
