import { Drawer, DrawerClose, DrawerContent, DrawerDescription, DrawerTitle } from "@ui-sdk/react/drawer"

function age(seconds) {
  return new Date(seconds * 1000).toLocaleString()
}

function Status({ node, describing }) {
  if (describing) return <p className="status">Writing a description…</p>
  if (node.kind === "file") return null
  if (node.summary === null) return <p className="status">No description yet.</p>
  if (node.description_stale) return <p className="status warn">The code has changed since this was written.</p>
  return null
}

/** Description, key symbols and the equivalent command for the selected node. */
export function SidePanel({ node, describing, onClose, onDescribe }) {
  return (
    <Drawer open={node !== null} onOpenChange={(open) => !open && onClose()} modal={false} side="end" closeOnOutsidePress={false}>
      <DrawerContent className="side-panel">
        {node && (
          <>
            <DrawerTitle>{node.name}</DrawerTitle>
            <DrawerDescription>
              {node.kind === "file" ? "File" : `Level ${node.level}`}
              {node.stale ? " · out of date" : ""}
            </DrawerDescription>
            <Status node={node} describing={describing} />
            {node.summary && <p>{node.summary}</p>}
            {node.tags.length > 0 && (
              <ul className="tags">
                {node.tags.map((tag) => (
                  <li key={tag}>{tag}</li>
                ))}
              </ul>
            )}
            {node.written_at && <p className="muted">Described {age(node.written_at)}</p>}
            {node.kind === "component" && (
              <button type="button" disabled={describing} onClick={onDescribe}>
                {node.summary === null ? "Write description" : "Refresh description"}
              </button>
            )}
            {node.key_symbols.length > 0 && (
              <section>
                <h3>Key symbols</h3>
                <ul className="symbols">
                  {node.key_symbols.map((symbol) => (
                    <li key={`${symbol.qualified_name}:${symbol.line}`}>
                      <code>{symbol.qualified_name}</code>
                      <span className="muted">
                        {symbol.kind}
                        {symbol.file ? ` · ${symbol.file}:${symbol.line ?? ""}` : ""}
                      </span>
                    </li>
                  ))}
                </ul>
              </section>
            )}
            <section>
              <h3>Same from the terminal</h3>
              <code className="command">{node.command}</code>
            </section>
            <DrawerClose>Close</DrawerClose>
          </>
        )}
      </DrawerContent>
    </Drawer>
  )
}
