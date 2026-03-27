// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	site: 'https://sacha-ops.github.io',
	base: '/syfrah',
	integrations: [
		starlight({
			title: 'Syfrah',
			social: [
				{ icon: 'github', label: 'GitHub', href: 'https://github.com/sacha-ops/syfrah' },
			],
			lastUpdated: true,
			sidebar: [
				{
					label: 'Overview',
					items: [
						{ label: 'Architecture', slug: 'handbook/architecture' },
					],
				},
				{
					label: 'Handbook',
					autogenerate: { directory: 'handbook' },
				},
				{
					label: 'Layers',
					autogenerate: { directory: 'layers' },
				},
				{
					label: 'API Reference',
					autogenerate: { directory: 'api' },
				},
				{
					label: 'Dev',
					autogenerate: { directory: 'dev' },
				},
				{
					label: 'Benchmarks',
					autogenerate: { directory: 'benchmarks' },
				},
				{
					label: 'Audits',
					autogenerate: { directory: 'audits' },
				},
			],
		}),
	],
});
