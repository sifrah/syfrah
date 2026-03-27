'use client'

import Link from 'next/link'

export function PageTags({ tags }: { tags: string[] }) {
  if (!tags || tags.length === 0) {
    return null
  }

  return (
    <div className="mt-2 flex flex-wrap gap-2">
      {tags.map((tag) => (
        <Link
          key={tag}
          href={`/?tag=${encodeURIComponent(tag)}`}
          className="inline-flex items-center rounded-full bg-blue-50 px-2.5 py-0.5 text-xs font-medium text-blue-700 ring-1 ring-inset ring-blue-600/20 transition hover:bg-blue-100 dark:bg-blue-400/10 dark:text-blue-400 dark:ring-blue-400/30 dark:hover:bg-blue-400/20"
        >
          {tag}
        </Link>
      ))}
    </div>
  )
}
