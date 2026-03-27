'use client'

import clsx from 'clsx'
import { AnimatePresence, motion, useIsPresent } from 'framer-motion'
import Link from 'next/link'
import { usePathname } from 'next/navigation'
import { useRef, useState } from 'react'

import { useIsInsideMobileNavigation } from '@/components/MobileNavigation'
import { useSectionStore } from '@/components/SectionProvider'
import { Tag } from '@/components/Tag'
import { remToPx } from '@/lib/remToPx'
import { CloseButton } from '@headlessui/react'

import navData from '@/navigation.json'

interface NavLink {
  title: string
  href: string
  children?: NavLink[]
}

interface NavGroup {
  title: string
  links: NavLink[]
}

function useInitialValue<T>(value: T, condition = true) {
  let initialValue = useRef(value).current
  return condition ? initialValue : value
}

function NavLinkItem({
  href,
  children,
  tag,
  active = false,
  isAnchorLink = false,
  depth = 0,
}: {
  href: string
  children: React.ReactNode
  tag?: string
  active?: boolean
  isAnchorLink?: boolean
  depth?: number
}) {
  return (
    <CloseButton
      as={Link}
      href={href}
      aria-current={active ? 'page' : undefined}
      className={clsx(
        'flex justify-between gap-2 py-1 pr-3 text-sm transition',
        isAnchorLink ? 'pl-7' : depth > 0 ? 'pl-8' : 'pl-4',
        active
          ? 'text-zinc-900 dark:text-white'
          : 'text-zinc-600 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-white',
      )}
    >
      <span className="truncate">{children}</span>
      {tag && (
        <Tag variant="small" color="zinc">
          {tag}
        </Tag>
      )}
    </CloseButton>
  )
}

function NavItemWithChildren({
  link,
  pathname,
}: {
  link: NavLink
  pathname: string
}) {
  let isChildActive = link.children?.some(
    (child) => child.href === pathname
  ) ?? false
  let isSelfActive = link.href === pathname
  let [isOpen, setIsOpen] = useState(isSelfActive || isChildActive)

  return (
    <motion.li layout="position" className="relative">
      <div className="flex items-center">
        <div className="flex-1">
          <NavLinkItem
            href={link.href}
            active={isSelfActive}
          >
            {link.title}
          </NavLinkItem>
        </div>
        <button
          onClick={() => setIsOpen(!isOpen)}
          className={clsx(
            'mr-2 flex h-5 w-5 items-center justify-center rounded text-zinc-500 transition hover:text-zinc-900 dark:hover:text-white',
          )}
          aria-label={isOpen ? 'Collapse' : 'Expand'}
        >
          <svg
            width="10"
            height="10"
            viewBox="0 0 10 10"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            className={clsx(
              'transition-transform duration-150',
              isOpen ? 'rotate-90' : '',
            )}
          >
            <path d="M3 1.5L7 5L3 8.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        </button>
      </div>
      <AnimatePresence initial={false}>
        {isOpen && link.children && link.children.length > 0 && (
          <motion.ul
            role="list"
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: 'auto' }}
            exit={{ opacity: 0, height: 0 }}
            className="overflow-hidden"
          >
            {link.children.map((child) => (
              <motion.li key={child.href} layout="position" className="relative">
                <NavLinkItem
                  href={child.href}
                  active={child.href === pathname}
                  depth={1}
                >
                  {child.title}
                </NavLinkItem>
              </motion.li>
            ))}
          </motion.ul>
        )}
      </AnimatePresence>
    </motion.li>
  )
}

function VisibleSectionHighlight({
  group,
  pathname,
}: {
  group: NavGroup
  pathname: string
}) {
  let [sections, visibleSections] = useInitialValue(
    [
      useSectionStore((s) => s.sections),
      useSectionStore((s) => s.visibleSections),
    ],
    useIsInsideMobileNavigation(),
  )

  let isPresent = useIsPresent()
  let firstVisibleSectionIndex = Math.max(
    0,
    [{ id: '_top' }, ...sections].findIndex(
      (section) => section.id === visibleSections[0],
    ),
  )
  let itemHeight = remToPx(2)
  let flatLinks = flattenLinks(group.links)
  let height = isPresent
    ? Math.max(1, visibleSections.length) * itemHeight
    : itemHeight
  let top =
    flatLinks.findIndex((link) => link.href === pathname) * itemHeight +
    firstVisibleSectionIndex * itemHeight

  return (
    <motion.div
      layout
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.2 } }}
      exit={{ opacity: 0 }}
      className="absolute inset-x-0 top-0 bg-zinc-800/2.5 will-change-transform dark:bg-white/2.5"
      style={{ borderRadius: 8, height, top }}
    />
  )
}

function ActivePageMarker({
  group,
  pathname,
}: {
  group: NavGroup
  pathname: string
}) {
  let itemHeight = remToPx(2)
  let offset = remToPx(0.25)
  let flatLinks = flattenLinks(group.links)
  let activePageIndex = flatLinks.findIndex((link) => link.href === pathname)
  let top = offset + activePageIndex * itemHeight

  return (
    <motion.div
      layout
      className="absolute left-2 h-6 w-px bg-blue-500"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1, transition: { delay: 0.2 } }}
      exit={{ opacity: 0 }}
      style={{ top }}
    />
  )
}

// Flatten nested links for index calculations
function flattenLinks(links: NavLink[]): NavLink[] {
  let result: NavLink[] = []
  for (let link of links) {
    result.push(link)
    if (link.children) {
      result.push(...link.children)
    }
  }
  return result
}

function NavigationGroup({
  group,
  className,
}: {
  group: NavGroup
  className?: string
}) {
  let isInsideMobileNavigation = useIsInsideMobileNavigation()
  let [pathname, sections] = useInitialValue(
    [usePathname(), useSectionStore((s) => s.sections)],
    isInsideMobileNavigation,
  )

  let flatLinks = flattenLinks(group.links)
  let isActiveGroup =
    flatLinks.findIndex((link) => link.href === pathname) !== -1

  return (
    <li className={clsx('relative mt-6', className)}>
      <motion.h2
        layout="position"
        className="text-xs font-semibold text-zinc-900 dark:text-white"
      >
        {group.title}
      </motion.h2>
      <div className="relative mt-3 pl-2">
        <motion.div
          layout
          className="absolute inset-y-0 left-2 w-px bg-zinc-900/10 dark:bg-white/5"
        />
        <AnimatePresence initial={false}>
          {isActiveGroup && (
            <ActivePageMarker group={group} pathname={pathname} />
          )}
        </AnimatePresence>
        <ul role="list" className="border-l border-transparent">
          {group.links.map((link) =>
            link.children && link.children.length > 0 ? (
              <NavItemWithChildren
                key={link.href}
                link={link}
                pathname={pathname}
              />
            ) : (
              <motion.li key={link.href} layout="position" className="relative">
                <NavLinkItem href={link.href} active={link.href === pathname}>
                  {link.title}
                </NavLinkItem>
                <AnimatePresence mode="popLayout" initial={false}>
                  {link.href === pathname && sections.length > 0 && (
                    <motion.ul
                      role="list"
                      initial={{ opacity: 0 }}
                      animate={{
                        opacity: 1,
                        transition: { delay: 0.1 },
                      }}
                      exit={{
                        opacity: 0,
                        transition: { duration: 0.15 },
                      }}
                    >
                      {sections.map((section) => (
                        <li key={section.id}>
                          <NavLinkItem
                            href={`${link.href}#${section.id}`}
                            tag={section.tag}
                            isAnchorLink
                          >
                            {section.title}
                          </NavLinkItem>
                        </li>
                      ))}
                    </motion.ul>
                  )}
                </AnimatePresence>
              </motion.li>
            ),
          )}
        </ul>
      </div>
    </li>
  )
}

// Build navigation groups from the auto-generated JSON
function buildNavigation(): NavGroup[] {
  let groups: NavGroup[] = []

  if (navData.overview?.length > 0) {
    groups.push({ title: 'Overview', links: navData.overview })
  }

  if (navData.layers?.length > 0) {
    groups.push({ title: 'Layers', links: navData.layers })
  }

  if (navData.handbook?.length > 0) {
    groups.push({ title: 'Handbook', links: navData.handbook })
  }

  // Dynamic extra groups (dev, benchmarks, post_release_audit, sdk, api, etc.)
  if (navData.extra) {
    for (const [, value] of Object.entries(navData.extra)) {
      const group = value as { title: string; links: NavLink[] }
      if (group.links?.length > 0) {
        groups.push({ title: group.title, links: group.links })
      }
    }
  }

  return groups
}

export const navigation = buildNavigation()

export function Navigation(props: React.ComponentPropsWithoutRef<'nav'>) {
  return (
    <nav {...props}>
      <ul role="list">
        {navigation.map((group, groupIndex) => (
          <NavigationGroup
            key={group.title}
            group={group}
            className={groupIndex === 0 ? 'md:mt-0' : ''}
          />
        ))}
      </ul>
    </nav>
  )
}
