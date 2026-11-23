import { useCallback, useEffect, useRef, useState } from "react"
import { describe, fetchLevel, fetchNode } from "./api.js"
import { Breadcrumb } from "./Breadcrumb.jsx"
import { Canvas } from "./Canvas.jsx"
import { ErrorDialog } from "./ErrorDialog.jsx"
import { SearchBox } from "./SearchBox.jsx"
import { SidePanel } from "./SidePanel.jsx"

const NO_BOXES = []

export function App() {
  const [parent, setParent] = useState(null)
  const [level, setLevel] = useState(null)
  const [selectedId, setSelectedId] = useState(null)
  const [node, setNode] = useState(null)
  const [describing, setDescribing] = useState(false)
  const [failure, setFailure] = useState(null)
  // Components already offered a description this session, so a failed model call is not retried in a loop.
  const attempted = useRef(new Set())

  const fail = useCallback((error) => setFailure(error.message), [])

  const loadLevel = useCallback(() => fetchLevel(parent).then(setLevel).catch(fail), [parent, fail])
  useEffect(() => {
    loadLevel()
  }, [loadLevel])

  const writeDescription = useCallback(
    (id) => {
      setDescribing(true)
      describe(id)
        .then((result) => {
          setNode(result.node)
          return loadLevel()
        })
        .catch(fail)
        .finally(() => setDescribing(false))
    },
    [fail, loadLevel],
  )

  useEffect(() => {
    if (selectedId === null) {
      setNode(null)
      return
    }
    fetchNode(selectedId)
      .then((loaded) => {
        setNode(loaded)
        if (loaded.kind === "component" && loaded.summary === null && !attempted.current.has(selectedId)) {
          attempted.current.add(selectedId)
          writeDescription(selectedId)
        }
      })
      .catch(fail)
  }, [selectedId, writeDescription, fail])

  const open = useCallback((box) => {
    setSelectedId(box.id)
    if (box.kind === "component") setParent(box.id)
  }, [])

  const goTo = useCallback((id) => {
    setParent(id)
    setSelectedId(id)
  }, [])

  const stale = level?.freshness.indexing_in_progress ? "Indexing in progress; showing the last committed snapshot." : null

  return (
    <div className="app">
      <header>
        <Breadcrumb crumbs={level?.breadcrumb ?? NO_BOXES} onGo={goTo} />
        <SearchBox onChoose={(hit) => goTo(hit.target)} />
      </header>
      {stale && <p className="banner">{stale}</p>}
      {level && level.boxes.length === 0 && <p className="empty">Nothing to show here. Run `cartograph index` and `cartograph cluster`.</p>}
      {level && level.hidden_boxes > 0 && (
        <p className="banner">{level.hidden_boxes} more boxes not drawn; search to find them.</p>
      )}
      <Canvas boxes={level?.boxes ?? NO_BOXES} arrows={level?.arrows ?? NO_BOXES} selectedId={selectedId} onOpen={open} />
      <SidePanel
        node={node}
        describing={describing}
        onClose={() => setSelectedId(null)}
        onDescribe={() => writeDescription(selectedId)}
      />
      <ErrorDialog message={failure} onClose={() => setFailure(null)} />
    </div>
  )
}
