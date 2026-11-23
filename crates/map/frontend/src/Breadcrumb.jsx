/** The path from the top level to the level on screen; each part goes back up to that level. */
export function Breadcrumb({ crumbs, onGo }) {
  return (
    <nav aria-label="Breadcrumb" className="breadcrumb">
      <ol>
        <li>
          <button type="button" onClick={() => onGo(null)} aria-current={crumbs.length === 0 ? "page" : undefined}>
            All systems
          </button>
        </li>
        {crumbs.map((crumb, index) => (
          <li key={crumb.id}>
            <button
              type="button"
              onClick={() => onGo(crumb.id)}
              aria-current={index === crumbs.length - 1 ? "page" : undefined}
            >
              {crumb.name}
            </button>
          </li>
        ))}
      </ol>
    </nav>
  )
}
