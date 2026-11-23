import Konva from "konva"
import { useEffect, useRef } from "react"
import { BOX_HEIGHT, BOX_WIDTH, arrowPoints, layout, strokeWidth } from "./layout.js"

const ZOOM_STEP = 1.1

function palette() {
  const style = getComputedStyle(document.documentElement)
  const read = (name) => style.getPropertyValue(name).trim()
  return {
    box: read("--box"),
    border: read("--box-border"),
    text: read("--text"),
    muted: read("--muted"),
    arrow: read("--arrow"),
    selected: read("--accent"),
  }
}

function drawBox(box, position, colors, selectedId, onOpen) {
  const group = new Konva.Group({ x: position.x, y: position.y, opacity: box.stale ? 0.5 : 1 })
  group.add(
    new Konva.Rect({
      width: BOX_WIDTH,
      height: BOX_HEIGHT,
      cornerRadius: 8,
      fill: colors.box,
      stroke: box.id === selectedId ? colors.selected : colors.border,
      strokeWidth: box.id === selectedId ? 3 : 1.5,
      dash: box.stale ? [6, 4] : [],
    }),
  )
  group.add(
    new Konva.Text({
      x: 12,
      y: 10,
      width: BOX_WIDTH - 24,
      height: 22,
      text: box.name,
      fontSize: 16,
      fontStyle: "bold",
      fill: colors.text,
      ellipsis: true,
      wrap: "none",
    }),
  )
  const detail = box.summary ?? `${box.file_count} ${box.file_count === 1 ? "file" : "files"}`
  group.add(
    new Konva.Text({
      x: 12,
      y: 38,
      width: BOX_WIDTH - 24,
      height: BOX_HEIGHT - 50,
      text: detail,
      fontSize: 13,
      fill: box.summary ? colors.text : colors.muted,
      ellipsis: true,
    }),
  )
  group.on("click tap", () => onOpen(box))
  group.on("mouseenter", () => (group.getStage().container().style.cursor = "pointer"))
  group.on("mouseleave", () => (group.getStage().container().style.cursor = "grab"))
  return group
}

function drawArrows(arrows, positions, colors) {
  const maxCount = Math.max(1, ...arrows.map((arrow) => arrow.count))
  return arrows.map((arrow) => {
    const width = strokeWidth(arrow.count, maxCount)
    return new Konva.Arrow({
      points: arrowPoints(positions.get(arrow.from), positions.get(arrow.to)),
      stroke: colors.arrow,
      fill: colors.arrow,
      strokeWidth: width,
      pointerLength: 6 + width,
      pointerWidth: 6 + width,
      opacity: 0.8,
    })
  })
}

/** The boxes and arrows of one level, on a pan-and-zoom canvas. */
export function Canvas({ boxes, arrows, selectedId, onOpen }) {
  const container = useRef(null)
  const stage = useRef(null)

  useEffect(() => {
    const instance = new Konva.Stage({
      container: container.current,
      width: container.current.clientWidth,
      height: container.current.clientHeight,
      draggable: true,
    })
    instance.container().style.cursor = "grab"
    instance.on("wheel", (event) => {
      event.evt.preventDefault()
      const pointer = instance.getPointerPosition()
      const before = instance.scaleX()
      const after = event.evt.deltaY < 0 ? before * ZOOM_STEP : before / ZOOM_STEP
      const anchor = {
        x: (pointer.x - instance.x()) / before,
        y: (pointer.y - instance.y()) / before,
      }
      instance.scale({ x: after, y: after })
      instance.position({ x: pointer.x - anchor.x * after, y: pointer.y - anchor.y * after })
    })
    const resize = new ResizeObserver(() =>
      instance.size({ width: container.current.clientWidth, height: container.current.clientHeight }),
    )
    resize.observe(container.current)
    stage.current = instance
    return () => {
      resize.disconnect()
      instance.destroy()
    }
  }, [])

  useEffect(() => {
    const instance = stage.current
    const colors = palette()
    const { positions, width } = layout(boxes, arrows)
    const content = new Konva.Layer()
    content.add(...drawArrows(arrows, positions, colors))
    for (const box of boxes) content.add(drawBox(box, positions.get(box.id), colors, selectedId, onOpen))
    instance.destroyChildren()
    instance.add(content)
    instance.scale({ x: 1, y: 1 })
    instance.position({ x: Math.max(0, (instance.width() - width) / 2), y: 0 })
  }, [boxes, arrows, selectedId, onOpen])

  return <div ref={container} className="canvas" data-box-count={boxes.length} />
}
