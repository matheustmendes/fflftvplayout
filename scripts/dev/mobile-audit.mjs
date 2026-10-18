// Auditoria de responsividade no celular (ver .dev/mobile_plan.md).
//
// Abre cada tela do painel numa janela de celular e mede: se a página estoura para o lado,
// quantos botões/campos têm menos de 36 px (difícil de tocar) e quantos textos têm menos de
// 12 px. Salva uma foto de cada tela. Os números são a linha de base das fases do plano.
//
// Precisa do playwright-core e de um Chromium, e de um arquivo de sessão já logada
// (storageState do Playwright). Exemplo:
//   npm i --no-save playwright-core
//   CHROMIUM_PATH=/usr/bin/chromium AUDIT_URL=http://127.0.0.1:8787 \
//     AUDIT_STATE=state-admin.json node scripts/dev/mobile-audit.mjs
// Variáveis: AUDIT_URL, AUDIT_STATE, AUDIT_OUT (pasta das fotos), AUDIT_WIDTH, AUDIT_HEIGHT,
// AUDIT_DETAILS=1 (lista os alvos pequenos e os textos pequenos de cada tela).
import { chromium } from 'playwright-core'
import fs from 'node:fs'
const OUT = process.env.AUDIT_OUT ?? './mobile-audit-shots'
const BASE = process.env.AUDIT_URL ?? 'http://127.0.0.1:8787'
const STATE = process.env.AUDIT_STATE ?? 'state-admin.json'
const WIDTH = Number(process.env.AUDIT_WIDTH ?? 390)
const HEIGHT = Number(process.env.AUDIT_HEIGHT ?? 844)
fs.mkdirSync(OUT, { recursive: true })
const routes = [
    ['painel', '/'],
    ['no-ar', '/player'],
    ['arquivos', '/media'],
    ['texto-na-tela', '/message'],
    ['cg', '/cg'],
    ['historico', '/logging'],
    ['pub-hoje', '/commercial/today'],
    ['pub-comerciais', '/commercial/spots'],
    ['pub-campanhas', '/commercial/campaigns'],
    ['pub-intervalos', '/commercial/breaks'],
    ['pub-grade', '/commercial/grid'],
    ['pub-relatorios', '/commercial/reports'],
    ['pub-prefs', '/commercial/settings'],
    ['aj-canal', '/configure/channel'],
    ['aj-transmissao', '/configure/playout'],
    ['aj-audio', '/configure/audio'],
    ['aj-gravacao', '/configure/recording'],
    ['aj-telegram', '/configure/telegram'],
    ['aj-usuarios', '/configure/user'],
    ['aj-sistema', '/configure/global'],
]
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH, args: ['--no-sandbox'] })
const ctx = await browser.newContext({
    storageState: STATE,
    viewport: { width: WIDTH, height: HEIGHT },
    hasTouch: true,
    isMobile: true,
    deviceScaleFactor: 1,
})
await ctx.addInitScript(() => {
    localStorage.setItem('language', 'pt-br')
    localStorage.setItem('theme', 'light')
})
const page = await ctx.newPage()
const errors = []
page.on('pageerror', (e) => errors.push(e.message))
for (const [name, path] of routes) {
    await page.goto(BASE + path)
    await page.waitForTimeout(1800)
    const r = await page.evaluate(() => {
        const vw = window.innerWidth
        const wide = []
        for (const e of document.querySelectorAll('body *')) {
            const b = e.getBoundingClientRect()
            if (b.width === 0 || b.height === 0) continue
            const cs = getComputedStyle(e)
            if (cs.position === 'fixed') continue
            if (b.right > vw + 1 && !e.closest('[class*="overflow-x"], .overflow-auto, table'))
                wide.push(
                    `${e.tagName.toLowerCase()}.${(e.className || '').toString().split(' ').slice(0, 3).join('.')}:${Math.round(b.right)}`,
                )
        }
        const describe = (e) => {
            const b = e.getBoundingClientRect()
            const text = (e.getAttribute('aria-label') || e.textContent || e.getAttribute('placeholder') || '')
                .trim()
                .replace(/\s+/g, ' ')
                .slice(0, 28)
            const cls = (e.className || '').toString().split(' ').slice(0, 3).join('.')
            return `${e.tagName.toLowerCase()}.${cls} "${text}" ${Math.round(b.width)}x${Math.round(b.height)}`
        }
        const visible = (e) => {
            const b = e.getBoundingClientRect()
            return b.width > 0 && b.height > 0 && getComputedStyle(e).visibility !== 'hidden'
        }
        const targets = [
            ...document.querySelectorAll('button, a, input:not([type=hidden]), select, textarea, [role=button]'),
        ].filter(
            (e) =>
                visible(e) &&
                !(e.type === 'checkbox' || e.type === 'radio') &&
                // O campo dentro de um .input é só o miolo: o alvo de toque é a caixa inteira.
                !e.parentElement?.classList.contains('input'),
        )
        const below = (limit) =>
            targets.filter((e) => {
                const b = e.getBoundingClientRect()
                return b.height < limit || b.width < limit
            })
        const tiny = [...document.querySelectorAll('body *')]
            .filter((e) => visible(e) && !e.closest('[data-audit-ignore]'))
            .filter(
                (e) =>
                    e.childNodes.length &&
                    [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) &&
                    parseFloat(getComputedStyle(e).fontSize) < 12,
            )
        const small = below(36)
        const under40 = below(40)
        return {
            scrollW: document.documentElement.scrollWidth,
            vw,
            overflowX: document.documentElement.scrollWidth > vw + 1,
            wide: wide.slice(0, 3),
            smallTargets: small.length,
            under40: under40.length,
            tinyText: tiny.length,
            details: { small: under40.slice(0, 12).map(describe), tiny: tiny.slice(0, 6).map(describe) },
        }
    })
    console.log(
        name.padEnd(16),
        r.overflowX ? 'ESTOURA' : 'ok     ',
        `scrollW=${r.scrollW}`,
        `alvos<36px=${r.smallTargets}`,
        `alvos<40px=${r.under40}`,
        `texto<12px=${r.tinyText}`,
        r.wide.length ? 'larg: ' + r.wide.join(' | ') : '',
    )
    if (process.env.AUDIT_DETAILS) {
        for (const line of r.details.small) console.log('    alvo  ', line)
        for (const line of r.details.tiny) console.log('    texto ', line)
    }
    await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: false })
}
console.log('erros de página:', JSON.stringify(errors))
await browser.close()
