'use client'

import { useEffect, useState } from 'react'
import clsx from 'clsx'

import { useSectionStore } from '@/components/SectionProvider'

interface TocEntry {
  id: string
  title: string
  level: number
}

export function TableOfContents() {
  let sections = useSectionStore((s) => s.sections)
  let visibleSections = useSectionStore((s) => s.visibleSections)
  let [headings, setHeadings] = useState<TocEntry[]>([])

  useEffect(() => {
    let entries: TocEntry[] = []
    let elements = document.querySelectorAll<HTMLHeadingElement>(
      'main h2[id], main h3[id]',
    )
    elements.forEach((el) => {
      entries.push({
        id: el.id,
        title: el.textContent ?? '',
        level: el.tagName === 'H3' ? 3 : 2,
      })
    })
    setHeadings(entries)
  }, [sections])

  if (headings.length < 3) {
    return null
  }

  return (
    <nav
      aria-label="Table of contents"
      className="hidden xl:block fixed right-0 top-16 bottom-0 w-56 overflow-y-auto px-4 py-8 2xl:w-64"
    >
      <h5 className="mb-3 text-xs font-semibold uppercase tracking-wide text-zinc-900 dark:text-white">
        On this page
      </h5>
      <ul className="space-y-2 text-sm">
        {headings.map((heading) => (
          <li key={heading.id} className={heading.level === 3 ? 'pl-3' : ''}>
            <a
              href={`#${heading.id}`}
              onClick={(e) => {
                e.preventDefault()
                let el = document.getElementById(heading.id)
                if (el) {
                  el.scrollIntoView({ behavior: 'smooth' })
                  window.history.replaceState(null, '', `#${heading.id}`)
                }
              }}
              className={clsx(
                'block leading-relaxed transition-colors',
                visibleSections.includes(heading.id)
                  ? 'font-medium text-emerald-500 dark:text-emerald-400'
                  : 'text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white',
              )}
            >
              {heading.title}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  )
}
