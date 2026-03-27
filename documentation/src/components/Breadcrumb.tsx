'use client'

import Link from 'next/link'
import { usePathname } from 'next/navigation'

function formatSegment(segment: string): string {
  return segment
    .split('-')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}

export function Breadcrumb() {
  let pathname = usePathname()

  if (pathname === '/') {
    return null
  }

  let segments = pathname.split('/').filter(Boolean)

  return (
    <nav aria-label="Breadcrumb" className="mb-4 text-sm">
      <ol className="flex items-center gap-1 text-zinc-500 dark:text-zinc-400">
        <li>
          <Link
            href="/"
            className="transition hover:text-zinc-900 dark:hover:text-white"
          >
            Home
          </Link>
        </li>
        {segments.map((segment, index) => {
          let href = '/' + segments.slice(0, index + 1).join('/')
          let isLast = index === segments.length - 1

          return (
            <li key={href} className="flex items-center gap-1">
              <span aria-hidden="true" className="text-zinc-400 dark:text-zinc-500">
                &gt;
              </span>
              {isLast ? (
                <span className="font-semibold text-zinc-900 dark:text-white">
                  {formatSegment(segment)}
                </span>
              ) : (
                <Link
                  href={href}
                  className="transition hover:text-zinc-900 dark:hover:text-white"
                >
                  {formatSegment(segment)}
                </Link>
              )}
            </li>
          )
        })}
      </ol>
    </nav>
  )
}
