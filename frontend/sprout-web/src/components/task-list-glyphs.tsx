import type { SVGProps } from 'react'

export type TaskListGlyphId =
  | 'heart'
  | 'star'
  | 'home'
  | 'user'
  | 'users'
  | 'check'
  | 'x'
  | 'plus'
  | 'minus'
  | 'search'
  | 'settings'
  | 'bell'
  | 'mail'
  | 'phone'
  | 'calendar'
  | 'clock'
  | 'flag'
  | 'bookmark'
  | 'tag'
  | 'folder'
  | 'file'
  | 'image'
  | 'camera'
  | 'video'
  | 'music'
  | 'mic'
  | 'map-pin'
  | 'compass'
  | 'globe'
  | 'sun'
  | 'moon'
  | 'cloud'
  | 'cloud-rain'
  | 'zap'
  | 'flame'
  | 'droplet'
  | 'leaf'
  | 'tree'
  | 'flower'
  | 'bug'
  | 'dog'
  | 'cat'
  | 'bird'
  | 'fish'
  | 'car'
  | 'bus'
  | 'train'
  | 'plane'
  | 'rocket'
  | 'ship'
  | 'bike'
  | 'anchor'
  | 'coffee'
  | 'pizza'
  | 'apple'
  | 'cake'
  | 'wine'
  | 'utensils'
  | 'shopping-bag'
  | 'gift'
  | 'credit-card'
  | 'wallet'
  | 'briefcase'
  | 'building'
  | 'hospital'
  | 'school'
  | 'book'
  | 'graduation-cap'
  | 'lightbulb'
  | 'key'
  | 'lock'
  | 'unlock'
  | 'shield'
  | 'target'
  | 'trophy'
  | 'medal'
  | 'dumbbell'
  | 'football'
  | 'basketball'
  | 'tennis'
  | 'gamepad'
  | 'puzzle'
  | 'palette'
  | 'brush'
  | 'scissors'
  | 'hammer'
  | 'wrench'
  | 'cpu'
  | 'smartphone'
  | 'laptop'
  | 'monitor'
  | 'printer'
  | 'wifi'
  | 'link'
  | 'paperclip'
  | 'archive'
  | 'trash'
  | 'edit'
  | 'eye'
  | 'smile'
  | 'frown'
  | 'meh'
  | 'thumbs-up'
  | 'thumbs-down'
  | 'message'
  | 'send'
  | 'inbox'
  | 'package'
  | 'truck'
  | 'activity'
  | 'bar-chart'
  | 'pie-chart'
  | 'trending-up'
  | 'layers'
  | 'grid'
  | 'list'
  | 'filter'
  | 'sort'
  | 'refresh'
  | 'download'
  | 'upload'
  | 'share'
  | 'copy'
  | 'clipboard'
  | 'pin'
  | 'sparkles'
  | 'wand'
  | 'gem'
  | 'crown'
  | 'ghost'
  | 'skull'
  | 'bandage'
  | 'pill'
  | 'thermometer'
  | 'umbrella'
  | 'mountain'
  | 'tent'
  | 'campfire'

type GlyphDefinition = {
  id: TaskListGlyphId
  label: string
  keywords: string[]
  paths: string[]
  circles?: Array<{ cx: number; cy: number; r: number }>
  rects?: Array<{ x: number; y: number; width: number; height: number; rx?: number }>
}

export const TASK_LIST_GLYPHS: GlyphDefinition[] = [
  { id: 'heart', label: 'Cuore', keywords: ['love', 'cuore'], paths: ['M12 21s-7-4.5-7-10a4 4 0 0 1 7-2 4 4 0 0 1 7 2c0 5.5-7 10-7 10Z'] },
  { id: 'star', label: 'Stella', keywords: ['star', 'stella'], paths: ['M12 3 14.5 9 21 9.5 16 14l1.5 7L12 17.5 6.5 21 8 14 3 9.5 9.5 9Z'] },
  { id: 'home', label: 'Casa', keywords: ['home', 'casa'], paths: ['M4 12 12 5l8 7', 'M6 11v9h12v-9'] },
  { id: 'user', label: 'Utente', keywords: ['user', 'person'], paths: ['M20 21a8 8 0 0 0-16 0'], circles: [{ cx: 12, cy: 8, r: 4 }] },
  { id: 'users', label: 'Gruppo', keywords: ['team', 'group'], paths: ['M17 21a4 4 0 0 0-8 0', 'M9 11a4 4 0 1 0-8 0', 'M23 11a3 3 0 1 0-6 0', 'M21 21a6 6 0 0 0-6-3'] },
  { id: 'check', label: 'Spunta', keywords: ['check', 'done'], paths: ['M5 12l5 5L20 7'] },
  { id: 'x', label: 'Chiudi', keywords: ['close', 'x'], paths: ['M6 6l12 12', 'M18 6 6 18'] },
  { id: 'plus', label: 'Più', keywords: ['add', 'plus'], paths: ['M12 5v14', 'M5 12h14'] },
  { id: 'minus', label: 'Meno', keywords: ['minus'], paths: ['M5 12h14'] },
  { id: 'search', label: 'Cerca', keywords: ['search'], paths: ['M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16Z', 'm21 21-4.3-4.3'] },
  { id: 'settings', label: 'Impostazioni', keywords: ['settings', 'gear'], paths: ['M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6Z', 'M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9c.26.604.852.997 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1Z'] },
  { id: 'bell', label: 'Campana', keywords: ['bell', 'notification'], paths: ['M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9', 'M13.73 21a2 2 0 0 1-3.46 0'] },
  { id: 'mail', label: 'Email', keywords: ['mail', 'email'], paths: ['M4 6h16v12H4z', 'm4 7 8 6 8-6'] },
  { id: 'phone', label: 'Telefono', keywords: ['phone'], paths: ['M22 16.92v3a2 2 0 0 1-2.18 2 19.79 19.79 0 0 1-8.63-3.07 19.5 19.5 0 0 1-6-6 19.79 19.79 0 0 1-3.07-8.67A2 2 0 0 1 4.11 2h3a2 2 0 0 1 2 1.72c.127.96.361 1.903.7 2.81a2 2 0 0 1-.45 2.11L8.09 9.91a16 16 0 0 0 6 6l1.27-1.27a2 2 0 0 1 2.11-.45c.907.339 1.85.573 2.81.7A2 2 0 0 1 22 16.92Z'] },
  { id: 'calendar', label: 'Calendario', keywords: ['calendar'], paths: ['M8 2v4', 'M16 2v4', 'M3 10h18', 'M5 6h14a2 2 0 0 1 2 2v12a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Z'] },
  { id: 'clock', label: 'Orologio', keywords: ['clock', 'time'], paths: ['M12 12V7', 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z'] },
  { id: 'flag', label: 'Bandiera', keywords: ['flag'], paths: ['M4 22V4', 'M4 4h14l-2 4 2 4H4'] },
  { id: 'bookmark', label: 'Segnalibro', keywords: ['bookmark'], paths: ['M6 4h12v17l-6-4-6 4Z'] },
  { id: 'tag', label: 'Etichetta', keywords: ['tag'], paths: ['M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82Z'], circles: [{ cx: 7, cy: 7, r: 1.5 }] },
  { id: 'folder', label: 'Cartella', keywords: ['folder'], paths: ['M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7l-2-2H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2Z'] },
  { id: 'file', label: 'File', keywords: ['file'], paths: ['M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z', 'M14 2v6h6'] },
  { id: 'image', label: 'Immagine', keywords: ['image', 'photo'], paths: ['M4 16l4.586-4.586a2 2 0 0 1 2.828 0L16 16', 'M14 7h.01', 'M4 20h16a2 2 0 0 0 2-2V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2Z'] },
  { id: 'camera', label: 'Fotocamera', keywords: ['camera'], paths: ['M14.5 4h-5L7 7H4a2 2 0 0 0-2 2v9a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-3l-2.5-3Z'], circles: [{ cx: 12, cy: 13, r: 3 }] },
  { id: 'video', label: 'Video', keywords: ['video'], paths: ['m16 13 5-3v8l-5-3V13Z', 'M4 7h10a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2Z'] },
  { id: 'music', label: 'Musica', keywords: ['music'], paths: ['M9 18V5l12-2v13', 'M9 18a3 3 0 1 0 0-6'], circles: [{ cx: 18, cy: 16, r: 3 }] },
  { id: 'mic', label: 'Microfono', keywords: ['mic'], paths: ['M12 14a3 3 0 0 0 3-3V5a3 3 0 1 0-6 0v6a3 3 0 0 0 3 3Z', 'M19 11a7 7 0 0 1-14 0', 'M12 18v3'] },
  { id: 'map-pin', label: 'Pin', keywords: ['pin', 'location'], paths: ['M12 22s7-4.35 7-11a7 7 0 1 0-14 0c0 6.65 7 11 7 11Z'], circles: [{ cx: 12, cy: 10, r: 2.5 }] },
  { id: 'compass', label: 'Bussola', keywords: ['compass'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M16.24 7.76 14.12 14.12 7.76 16.24l2.12-6.36 6.36-2.12Z'] },
  { id: 'globe', label: 'Mondo', keywords: ['globe', 'world'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M2 12h20', 'M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10Z'] },
  { id: 'sun', label: 'Sole', keywords: ['sun'], paths: ['M12 2v2', 'M12 20v2', 'M4.93 4.93l1.41 1.41', 'M17.66 17.66l1.41 1.41', 'M2 12h2', 'M20 12h2', 'M4.93 19.07l1.41-1.41', 'M17.66 6.34l1.41-1.41'], circles: [{ cx: 12, cy: 12, r: 4 }] },
  { id: 'moon', label: 'Luna', keywords: ['moon'], paths: ['M21 14.5A8.5 8.5 0 1 1 9.5 3a6.5 6.5 0 0 0 11.5 11.5Z'] },
  { id: 'cloud', label: 'Nuvola', keywords: ['cloud'], paths: ['M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10Z'] },
  { id: 'cloud-rain', label: 'Pioggia', keywords: ['rain'], paths: ['M18 10h-1.26A8 8 0 1 0 9 20h9a5 5 0 0 0 0-10Z', 'M8 22v-2', 'M12 22v-2', 'M16 22v-2'] },
  { id: 'zap', label: 'Fulmine', keywords: ['lightning', 'zap'], paths: ['M13 2 3 14h9l-1 8 10-12h-9l1-8Z'] },
  { id: 'flame', label: 'Fuoco', keywords: ['fire', 'flame'], paths: ['M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5Z'] },
  { id: 'droplet', label: 'Goccia', keywords: ['water', 'droplet'], paths: ['M12 22a7 7 0 0 0 7-7c0-2-1-3.9-3-6-2.2-2.3-4-4.6-4-7-3.5 3.9-6 7.5-6 11a7 7 0 0 0 6 9Z'] },
  { id: 'leaf', label: 'Foglia', keywords: ['leaf', 'nature'], paths: ['M11 20A7 7 0 0 1 9.8 6.1C15.5 5 17 4.48 19 2c1 2 2 4.18 2 8 0 5.5-4.78 10-10 10Z', 'M2 21c0-3 1.85-5.36 5.08-6C9.5 14.52 12 13 13 12'] },
  { id: 'tree', label: 'Albero', keywords: ['tree'], paths: ['M12 22v-4', 'M6 14h12', 'M8 14V9l4-5 4 5v5'] },
  { id: 'flower', label: 'Fiore', keywords: ['flower'], paths: ['M12 7.5a4.5 4.5 1 1 1 0 9 4.5 4.5 0 0 1 0-9Z', 'M12 2v2.5', 'M12 19.5V22', 'M4.22 4.22l1.77 1.77', 'M18.01 18.01l1.77 1.77', 'M2 12h2.5', 'M19.5 12H22', 'M4.22 19.78l1.77-1.77', 'M18.01 5.99l1.77-1.77'] },
  { id: 'bug', label: 'Insetto', keywords: ['bug'], paths: ['M8 2v4', 'M16 2v4', 'M12 22v-4', 'M6 12H2', 'M22 12h-4', 'M8 8l-4-2', 'M16 8l4-2', 'M8 16l-4 2', 'M16 16l4 2', 'M12 8a4 4 0 0 0-4 4v4h8v-4a4 4 0 0 0-4-4Z'] },
  { id: 'dog', label: 'Cane', keywords: ['dog'], paths: ['M10 5.172C10 3.782 8.423 2.679 6.5 3c-2.823.47-4.113 6.006-4 7 .137 1.217 1.088 2.28 2.5 2.813', 'M14 5.172C14 3.782 15.577 2.679 17.5 3c2.823.47 4.113 6.006 4 7-.137 1.217-1.088 2.28-2.5 2.813', 'M8 14v.5', 'M16 14v.5', 'M11.25 16.25h1.5L12 17l-.75-.75Z', 'M4.42 11.247A13.152 13.152 0 0 0 4 14.556C4 18.728 7.582 21 12 21s8-2.272 8-6.444a13.15 13.15 0 0 0-.42-3.309'] },
  { id: 'cat', label: 'Gatto', keywords: ['cat'], paths: ['M12 5c.67 0 1.35.09 2 .26 1.78-2 5.03-2 5.03-2 0 1.98-1.16 3.7-2.88 4.5 1.6.26 3.1 1.04 4.35 2.24-1.4 1.26-3.27 2-5.2 2H6.5c-1.93 0-3.8-.74-5.2-2 1.25-1.2 2.75-1.98 4.35-2.24C3.84 6.7 2.68 4.98 2.68 3c0 0 3.25 0 5.03 2 .65-.17 1.33-.26 2-.26Z', 'M8 14h.01', 'M16 14h.01'] },
  { id: 'bird', label: 'Uccello', keywords: ['bird'], paths: ['M16 7h.01', 'M3 11c1.5-2 4-3.5 6.5-3.5 2.2 0 4 1 5.5 2.5', 'M3 11c0 5 4.5 9 10 9 2.5 0 4.8-1 6.5-2.5', 'M3 11l4-2', 'M21 8.5l-4 2.5'] },
  { id: 'fish', label: 'Pesce', keywords: ['fish'], paths: ['M6.5 12c.94-3.46 4.94-6 8.5-6 3 0 5.5 2.5 5.5 5.5S18 17 15 17c-3.56 0-7.56-2.54-8.5-6Z', 'M6.5 12H2', 'M22 12h-1'] },
  { id: 'car', label: 'Auto', keywords: ['car'], paths: ['M19 17h2c.6 0 1-.4 1-1v-3c0-.9-.7-1.7-1.5-1.9L18 8H6L4.5 11.1C3.7 11.3 3 12.1 3 13v3c0 .6.4 1 1 1h2', 'M7 17h10'], circles: [{ cx: 7, cy: 17, r: 2 }, { cx: 17, cy: 17, r: 2 }] },
  { id: 'bus', label: 'Bus', keywords: ['bus'], paths: ['M8 6v6', 'M15 6v6', 'M2 12h19.6', 'M18 18h3s-.5-1.7-1-2.5c-.4-.7-1-1.2-1.8-1.2H5.8c-.8 0-1.4.5-1.8 1.2-.5.8-1 2.5-1 2.5h3'], circles: [{ cx: 7, cy: 18, r: 2 }, { cx: 17, cy: 18, r: 2 }] },
  { id: 'train', label: 'Treno', keywords: ['train'], paths: ['M4 15h16', 'M4 15v3', 'M20 15v3', 'M4 11h16V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v5Z'], circles: [{ cx: 8, cy: 18, r: 1.5 }, { cx: 16, cy: 18, r: 1.5 }] },
  { id: 'plane', label: 'Aereo', keywords: ['plane'], paths: ['M17.8 19.2 16 11l3.5-3.5C21 6 21.5 4 19 4c-1 0-2 1-3.5 2.5L12 10 3.8 8.2c-.5-.1-.9.1-1.1.5l-.3.5c-.2.5-.1 1 .3 1.3L9 12l-2 3H4l-1 1 3 2 2 3 1-1v-3l3-2 3.5 5.3c.3.4.8.5 1.3.3l.5-.2c.4-.3.6-.7.5-1.2Z'] },
  { id: 'rocket', label: 'Razzo', keywords: ['rocket'], paths: ['M4.5 16.5c-1.5 1.26-2 5-2 5s3.74-.5 5-2c.71-.84.7-2.13-.09-2.91a2.18 2.18 0 0 0-2.91-.09Z', 'M12 15l-3-3a22 22 0 0 1 2-3.95A12.88 12.88 0 0 1 22 2c0 2.72-.78 7.5-6 11a22.35 22.35 0 0 1-4 2Z', 'M9 12H4s.55-3.03 2-4c1.62-1.08 5 0 5 0', 'M12 15v5s3.03-.55 4-2c1.08-1.62 0-5 0-5'] },
  { id: 'ship', label: 'Nave', keywords: ['ship', 'boat'], paths: ['M2 21c.6.5 1.2 1 2.5 1 2.5 0 2.5-2 5-2 2.5 0 2.5 2 5 2 2.5 0 2.5-2 5-2 1.3 0 1.9.5 2.5 1', 'M19.38 20A11.6 11.6 0 0 0 21 14l-9-4-9 4c0 2.9.94 5.34 2.81 7.76'] },
  { id: 'bike', label: 'Bici', keywords: ['bike'], paths: ['M18.5 19.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z', 'M5.5 19.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7Z', 'M15 6h5l-3 5', 'M12 17.5V14l-3-3 4-3 2 3h2'] },
  { id: 'anchor', label: 'Ancora', keywords: ['anchor'], paths: ['M12 22V8', 'M5 12H2a10 10 0 0 0 20 0h-3', 'M12 2v2', 'M7 8a5 5 0 0 1 10 0'] },
  { id: 'coffee', label: 'Caffè', keywords: ['coffee'], paths: ['M18 8h1a4 4 0 0 1 0 8h-1', 'M2 8h16v9a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V8Z', 'M6 1v3', 'M10 1v3', 'M14 1v3'] },
  { id: 'pizza', label: 'Pizza', keywords: ['pizza'], paths: ['M15 11h.01', 'M11 15h.01', 'M16 16h.01', 'm2 16 20-6.5a.5.5 0 0 0 0-.93L3.662 2.62a.5.5 0 0 0-.923.24l-1.2 12.5A2 2 0 0 0 3.76 17.5Z'] },
  { id: 'apple', label: 'Mela', keywords: ['apple', 'fruit'], paths: ['M12 20.94c1.5 0 2.75 1.06 4 1.06 3 0 6-8 6-12.22A4.91 4.91 0 0 0 17 5c-2.22 0-4 1.44-5 2-1-.56-2.78-2-5-2a4.9 4.9 0 0 0-5 4.78C2 14 5 22 8 22c1.25 0 2.5-1.06 4-1.06Z', 'M12 7c0-1.1.9-2 2-2'] },
  { id: 'cake', label: 'Torta', keywords: ['cake', 'birthday'], paths: ['M20 21v-8a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8', 'M4 16s.5-1 2-1 2.5 2 4 2 2.5-2 4-2 2.5 2 4 2 2-1 2-1', 'M2 21h20', 'M7 8v3', 'M12 8v3', 'M17 8v3', 'M7 4h.01', 'M12 4h.01', 'M17 4h.01'] },
  { id: 'wine', label: 'Bicchiere', keywords: ['wine', 'drink'], paths: ['M8 22h8', 'M12 11v11', 'M8 2h8l-1 9a4 4 0 0 1-6 0Z'] },
  { id: 'utensils', label: 'Posate', keywords: ['food', 'restaurant'], paths: ['M3 2v7c0 1.1.9 2 2 2h0a2 2 0 0 0 2-2V2', 'M7 2v20', 'M21 15V2v0a5 5 0 0 0-5 5v6c0 1.1.9 2 2 2h3Zm0 0v7'] },
  { id: 'shopping-bag', label: 'Shopping', keywords: ['shop', 'bag'], paths: ['M6 2 3 6v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V6l-3-4Z', 'M3 6h18', 'M16 10a4 4 0 0 1-8 0'] },
  { id: 'gift', label: 'Regalo', keywords: ['gift'], paths: ['M20 12v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8', 'M2 7h20v5H2z', 'M12 22V7', 'M12 7H7.5a2.5 2.5 0 0 1 0-5C11 2 12 7 12 7Z', 'M12 7h4.5a2.5 2.5 0 0 0 0-5C13 2 12 7 12 7Z'] },
  { id: 'credit-card', label: 'Carta', keywords: ['card', 'payment'], paths: ['M2 8h20v8a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V8Z', 'M2 10h20'] },
  { id: 'wallet', label: 'Portafoglio', keywords: ['wallet'], paths: ['M21 12V7H5a2 2 0 0 1 0-4h14v4', 'M3 5v14a2 2 0 0 0 2 2h16v-5', 'M18 12a2 2 0 0 0 0 4h4v-4Z'] },
  { id: 'briefcase', label: 'Valigetta', keywords: ['work', 'briefcase'], paths: ['M10 2h4', 'M4 7h16v12a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V7Z', 'M2 12h20'] },
  { id: 'building', label: 'Edificio', keywords: ['building', 'office'], paths: ['M6 22V4a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v18Z', 'M6 12H4a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2h2', 'M18 9h2a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2h-2', 'M10 6h4', 'M10 10h4', 'M10 14h4', 'M10 18h4'] },
  { id: 'hospital', label: 'Ospedale', keywords: ['hospital', 'health'], paths: ['M12 6v4', 'M10 8h4', 'M3 21h18', 'M5 21V7l7-4 7 4v14'] },
  { id: 'school', label: 'Scuola', keywords: ['school'], paths: ['M22 10v6', 'M2 10l10-5 10 5-10 5z', 'M6 12v5c3 3 9 3 12 0v-5'] },
  { id: 'book', label: 'Libro', keywords: ['book', 'read'], paths: ['M4 19.5A2.5 2.5 0 0 1 6.5 17H20', 'M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2Z'] },
  { id: 'graduation-cap', label: 'Laurea', keywords: ['graduation'], paths: ['M22 10v6M2 10l10-5 10 5-10 5z', 'M6 12v5c3 3 9 3 12 0v-5'] },
  { id: 'lightbulb', label: 'Idea', keywords: ['idea', 'lightbulb'], paths: ['M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5', 'M9 18h6', 'M10 22h4'] },
  { id: 'key', label: 'Chiave', keywords: ['key'], paths: ['M21 2l-2 2m-7.61 7.61a5.5 5.5 0 1 1-7.778 7.778 5.5 5.5 0 0 1 7.777-7.777Zm0 0L15.5 7.5m0 0 3 3L22 7l-3-3m-3.5 3.5L19 4'] },
  { id: 'lock', label: 'Lucchetto', keywords: ['lock'], paths: ['M7 11V7a5 5 0 0 1 10 0v4', 'M5 11h14v10H5z'] },
  { id: 'unlock', label: 'Aperto', keywords: ['unlock'], paths: ['M7 11V7a5 5 0 0 1 9.9-1', 'M5 11h14v10H5z'] },
  { id: 'shield', label: 'Scudo', keywords: ['shield', 'security'], paths: ['M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z'] },
  { id: 'target', label: 'Bersaglio', keywords: ['target'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M12 18a6 6 0 1 0 0-12 6 6 0 0 0 0 12Z', 'M12 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4Z'] },
  { id: 'trophy', label: 'Trofeo', keywords: ['trophy', 'win'], paths: ['M8 21h8', 'M12 17v4', 'M7 4h10', 'M17 4h2a2 2 0 0 1 2 2v2a4 4 0 0 1-4 4h-1', 'M7 4H5a2 2 0 0 0-2 2v2a4 4 0 0 0 4 4h1', 'M7 8h10'] },
  { id: 'medal', label: 'Medaglia', keywords: ['medal'], paths: ['M7.21 15 2.66 7.14a2 2 0 0 1 .13-2.2L4.4 2.8A2 2 0 0 1 6 2h12a2 2 0 0 1 1.6.8l1.6 2.14a2 2 0 0 1 .14 2.2L16.79 15', 'M11 12a3 3 0 1 0 6 0 3 3 0 0 0-6 0Z'] },
  { id: 'dumbbell', label: 'Pesi', keywords: ['gym', 'fitness'], paths: ['M6.5 6.5h11v11h-11z', 'M6.5 6.5 3 3', 'M17.5 6.5 21 3', 'M6.5 17.5 3 21', 'M17.5 17.5 21 21'] },
  { id: 'football', label: 'Calcio', keywords: ['football', 'soccer'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M12 2l3 5 5 1-3.5 4 1 5-5.5-3-5.5 3-1-5 3.5-4-5-1 3-5Z'] },
  { id: 'basketball', label: 'Basket', keywords: ['basketball'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M2 12h20', 'M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10Z'] },
  { id: 'tennis', label: 'Tennis', keywords: ['tennis'], paths: ['M14.5 2.5c-1 2.5-1 5.5 0 8 1.5 3 4 5 7 6.5', 'M2.5 14.5c2.5-1 5.5-1 8 0 3 1.5 5 4 6.5 7', 'M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z'] },
  { id: 'gamepad', label: 'Game', keywords: ['game', 'play'], paths: ['M6 12h4', 'M8 10v4', 'M15 13h.01', 'M18 11h.01', 'M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.042.256-.042.514 0 .77A4 4 0 0 0 6.68 13h10.64a4 4 0 0 0 3.978-3.59 4 4 0 0 0 0-.77A4 4 0 0 0 17.32 5Z'] },
  { id: 'puzzle', label: 'Puzzle', keywords: ['puzzle'], paths: ['M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 0 1-.837.276c-.47-.07-.802-.48-.968-.925a2.501 2.501 0 1 0-3.214 3.214c-.446.166-.855.497-.925.968a.979.979 0 0 1-.276.837l-1.61 1.61a2.404 2.404 0 0 1-1.705.707 2.402 2.402 0 0 1-1.704-.706l-1.568-1.568a1.026 1.026 0 0 0-.877-.29 2.501 2.501 0 1 0-2.7 2.7 1.026 1.026 0 0 0 .29.877l1.568 1.568a2.404 2.404 0 0 1 .706 1.704 2.402 2.402 0 0 1-1.704.706 2.404 2.404 0 0 1-1.705-.707l-1.611-1.61a.98.98 0 0 1-.276-.837c.07-.47.48-.802.925-.968a2.501 2.501 0 1 0-3.214-3.214c-.445.166-.855.497-.925.968a.979.979 0 0 1-.276-.837l-1.61-1.611A2.404 2.404 0 0 1 2.3 12.89a2.402 2.402 0 0 1 .706-1.704l1.568-1.568a1.026 1.026 0 0 0 .29-.877 2.501 2.501 0 1 0 2.7-2.7 1.026 1.026 0 0 0-.877-.29 2.404 2.404 0 0 1-1.704-.706 2.402 2.402 0 0 1 .706-1.704l1.611-1.61a.98.98 0 0 1 .837-.276c.47.07.802.48.968.925a2.501 2.501 0 1 0 3.214-3.214c-.166-.446-.497-.855-.968-.925a.979.979 0 0 1 .276-.837l1.61-1.611A2.404 2.404 0 0 1 12.89 2.3a2.402 2.402 0 0 1 1.704.706l1.568 1.568c.23.23.556.338.878.289a2.501 2.501 0 1 0 2.7 2.7Z'] },
  { id: 'palette', label: 'Tavolozza', keywords: ['palette', 'color'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M8 14h.01', 'M12 8h.01', 'M16 14h.01', 'M9 11h.01'] },
  { id: 'brush', label: 'Pennello', keywords: ['brush', 'paint'], paths: ['M9.06 11.9 8 16l4-1.06 8.12-8.12a2.5 2.5 0 0 0-3.54-3.54L9.06 11.9Z', 'M3 21l2.5-2.5'] },
  { id: 'scissors', label: 'Forbici', keywords: ['scissors'], paths: ['M6 6a3 3 0 1 0 6 0 3 3 0 0 0-6 0Z', 'M6 18a3 3 0 1 0 6 0 3 3 0 0 0-6 0Z', 'M20 4 8.12 15.88', 'M14.47 14.48 20 20', 'M8.12 8.12 12 12'] },
  { id: 'hammer', label: 'Martello', keywords: ['hammer', 'tool'], paths: ['M15 12l-8.5 8.5c-.83.83-2.17.83-3 0 0 0 0 0 0 0a2.12 2.12 0 0 1 0-3L12 9', 'M17.64 15 22 10.64', 'M20.91 11.7a1.69 1.69 0 0 0-.05-2.39l-1.32-1.32a1.69 1.69 0 0 0-2.39-.05L15 9.64'] },
  { id: 'wrench', label: 'Chiave inglese', keywords: ['wrench'], paths: ['M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76Z'] },
  { id: 'cpu', label: 'Chip', keywords: ['cpu', 'tech'], paths: ['M9 2v2', 'M15 2v2', 'M9 20v2', 'M15 20v2', 'M2 9h2', 'M2 15h2', 'M20 9h2', 'M20 15h2', 'M7 7h10v10H7z'] },
  { id: 'smartphone', label: 'Telefono', keywords: ['phone', 'mobile'], paths: ['M6 2h12v20H6z', 'M10 18h4'] },
  { id: 'laptop', label: 'Laptop', keywords: ['laptop', 'computer'], paths: ['M4 6h16v10H4z', 'M2 18h20'] },
  { id: 'monitor', label: 'Monitor', keywords: ['monitor', 'screen'], paths: ['M4 4h16v12H4z', 'M8 20h8', 'M12 16v4'] },
  { id: 'printer', label: 'Stampante', keywords: ['printer'], paths: ['M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2', 'M6 9V3h12v6', 'M6 14h12v7H6z'] },
  { id: 'wifi', label: 'Wi‑Fi', keywords: ['wifi'], paths: ['M5 12.55a11 11 0 0 1 14.08 0', 'M8.53 16.11a6 6 0 0 1 6.95 0', 'M12 20h.01'] },
  { id: 'link', label: 'Link', keywords: ['link'], paths: ['M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71', 'M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71'] },
  { id: 'paperclip', label: 'Graffetta', keywords: ['attach'], paths: ['M21.44 11.05l-9.19 9.19a6 6 0 0 1-8.49-8.49l9.19-9.19a4 4 0 0 1 5.66 5.66l-9.2 9.19a2 2 0 0 1-2.83-2.83l8.49-8.48'] },
  { id: 'archive', label: 'Archivio', keywords: ['archive'], paths: ['M3 3h18v4H3z', 'M5 7v12h14V7', 'M10 12h4'] },
  { id: 'trash', label: 'Cestino', keywords: ['trash', 'delete'], paths: ['M3 6h18', 'M8 6V4h8v2', 'M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6', 'M10 11v6', 'M14 11v6'] },
  { id: 'edit', label: 'Modifica', keywords: ['edit', 'pencil'], paths: ['M12 20h9', 'M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4Z'] },
  { id: 'eye', label: 'Occhio', keywords: ['eye', 'view'], paths: ['M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z'], circles: [{ cx: 12, cy: 12, r: 3 }] },
  { id: 'smile', label: 'Sorriso', keywords: ['smile', 'happy'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M8 14s1.5 2 4 2 4-2 4-2', 'M9 9h.01', 'M15 9h.01'] },
  { id: 'frown', label: 'Triste', keywords: ['sad', 'frown'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M16 16s-1.5-2-4-2-4 2-4 2', 'M9 9h.01', 'M15 9h.01'] },
  { id: 'meh', label: 'Neutro', keywords: ['meh', 'neutral'], paths: ['M12 22a10 10 0 1 0 0-20 10 10 0 0 0 0 20Z', 'M8 15h8', 'M9 9h.01', 'M15 9h.01'] },
  { id: 'thumbs-up', label: 'Like', keywords: ['like', 'thumbs'], paths: ['M7 10v12', 'M15 5.88 14 10h5.83a2 2 0 0 1 1.92 2.56l-2.33 8A2 2 0 0 1 17.5 22H4a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2h2.76a2 2 0 0 0 1.79-1.11L12 2h0a3.13 3.13 0 0 1 3 3.88Z'] },
  { id: 'thumbs-down', label: 'Dislike', keywords: ['dislike'], paths: ['M17 14V2', 'M9 18.12 10 14H4.17a2 2 0 0 1-1.92-2.56l2.33-8A2 2 0 0 1 6.5 2H20a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-2.76a2 2 0 0 0-1.79 1.11L12 22h0a3.13 3.13 0 0 1-3-3.88Z'] },
  { id: 'message', label: 'Messaggio', keywords: ['message', 'chat'], paths: ['M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2Z'] },
  { id: 'send', label: 'Invia', keywords: ['send'], paths: ['M22 2 11 13', 'M22 2 15 22l-4-9-9-4Z'] },
  { id: 'inbox', label: 'Inbox', keywords: ['inbox'], paths: ['M22 12h-6l-2 3H10l-2-3H2', 'M5 4h14v16H5z'] },
  { id: 'package', label: 'Pacco', keywords: ['package'], paths: ['M16.5 9.4 7.55 4.24', 'M21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16Z', 'M3.3 7 12 12l8.7-5', 'M12 22V12'] },
  { id: 'truck', label: 'Camion', keywords: ['truck', 'delivery'], paths: ['M14 18V6a2 2 0 0 0-2-2H4a2 2 0 0 0-2 2v11a1 1 0 0 0 1 1h2', 'M15 18H9', 'M19 18h2a1 1 0 0 0 1-1v-3.65a1 1 0 0 0-.22-.624l-3.48-4.35A1 1 0 0 0 17.52 8H14', 'M17 18a2 2 0 1 0 4 0 2 2 0 0 0-4 0Z', 'M7 18a2 2 0 1 0 4 0 2 2 0 0 0-4 0Z'] },
  { id: 'activity', label: 'Attività', keywords: ['activity', 'pulse'], paths: ['M22 12h-4l-3 9L9 3l-3 9H2'] },
  { id: 'bar-chart', label: 'Grafico', keywords: ['chart', 'bar'], paths: ['M12 20V10', 'M18 20V4', 'M6 20v-4'] },
  { id: 'pie-chart', label: 'Torta chart', keywords: ['pie', 'chart'], paths: ['M21.21 15.89A10 10 0 1 1 8 2.83', 'M22 12A10 10 0 0 0 12 2v10Z'] },
  { id: 'trending-up', label: 'Trend', keywords: ['trend', 'growth'], paths: ['M22 7 13.5 15.5 8.5 10.5 2 17', 'M16 7h6v6'] },
  { id: 'layers', label: 'Layers', keywords: ['layers', 'stack'], paths: ['M12 2 2 7l10 5 10-5-10-5Z', 'M2 17l10 5 10-5', 'M2 12l10 5 10-5'] },
  { id: 'grid', label: 'Griglia', keywords: ['grid'], paths: ['M3 3h7v7H3z', 'M14 3h7v7h-7z', 'M14 14h7v7h-7z', 'M3 14h7v7H3z'] },
  { id: 'list', label: 'Lista', keywords: ['list'], paths: ['M8 6h13', 'M8 12h13', 'M8 18h13', 'M3 6h.01', 'M3 12h.01', 'M3 18h.01'] },
  { id: 'filter', label: 'Filtro', keywords: ['filter'], paths: ['M22 3H2l8 9.46V19l4 2v-8.54L22 3Z'] },
  { id: 'sort', label: 'Ordina', keywords: ['sort'], paths: ['M11 5h10', 'M11 9h7', 'M11 13h4', 'M3 17h18', 'M3 13h1.5', 'M3 9h3', 'M3 5h4.5'] },
  { id: 'refresh', label: 'Aggiorna', keywords: ['refresh'], paths: ['M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8', 'M3 3v5h5', 'M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16', 'M16 16h5v5'] },
  { id: 'download', label: 'Download', keywords: ['download'], paths: ['M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4', 'M7 10l5 5 5-5', 'M12 15V3'] },
  { id: 'upload', label: 'Upload', keywords: ['upload'], paths: ['M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4', 'M17 8l-5-5-5 5', 'M12 3v12'] },
  { id: 'share', label: 'Condividi', keywords: ['share'], paths: ['M4 12v8a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-8', 'M16 6l-4-4-4 4', 'M12 2v13'] },
  { id: 'copy', label: 'Copia', keywords: ['copy'], paths: ['M16 16H6a2 2 0 0 1-2-2V6', 'M8 8h10a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2Z'] },
  { id: 'clipboard', label: 'Appunti', keywords: ['clipboard'], paths: ['M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2', 'M9 2h6v4H9z'] },
  { id: 'pin', label: 'Pin', keywords: ['pin'], paths: ['M12 17v5', 'M9 3h6l1 7 4 2v3H4v-3l4-2 1-7Z'] },
  { id: 'sparkles', label: 'Sparkle', keywords: ['sparkle', 'magic'], paths: ['M9.94 9.94 4 4', 'M14.06 9.94 20 4', 'M9.94 14.06 4 20', 'M14.06 14.06 20 20', 'M12 2v2', 'M12 20v2', 'M2 12h2', 'M20 12h2'] },
  { id: 'wand', label: 'Bacchetta', keywords: ['wand', 'magic'], paths: ['M15 4V2', 'M15 16v-2', 'M8 9h2', 'M20 9h2', 'M17.8 11.8 19 13', 'M15 9h0', 'M17.8 6.2 19 5', 'M3 21l9-9'] },
  { id: 'gem', label: 'Gemma', keywords: ['gem', 'diamond'], paths: ['M6 3h12l4 6-10 13L2 9Z', 'M2 9h20', 'M12 22 6 9l6-6 6 6-6 13Z'] },
  { id: 'crown', label: 'Corona', keywords: ['crown'], paths: ['M2 18h20', 'M2 14l3-8 5 4 5-6 5 6 3-8'] },
  { id: 'ghost', label: 'Fantasma', keywords: ['ghost'], paths: ['M9 10h.01', 'M15 10h.01', 'M12 2a8 8 0 0 0-8 8v12l3-2 3 2 3-2 3 2 3-2 3 2V10a8 8 0 0 0-8-8Z'] },
  { id: 'skull', label: 'Teschio', keywords: ['skull'], paths: ['M12 22a8 8 0 0 0 8-8V9a8 8 0 1 0-16 0v5a8 8 0 0 0 8 8Z', 'M9 14h.01', 'M15 14h.01', 'M9 18h6'] },
  { id: 'bandage', label: 'Cerotto', keywords: ['bandage', 'health'], paths: ['M12 12v.01', 'M18 6h-1.586a2 2 0 0 0-1.414.586l-7.172 7.172a2 2 0 0 0-.586 1.414V18a2 2 0 0 0 2 2h1.586a2 2 0 0 0 1.414-.586l7.172-7.172a2 2 0 0 0 .586-1.414V6a2 2 0 0 0-2-2Z'] },
  { id: 'pill', label: 'Pillola', keywords: ['pill', 'medicine'], paths: ['M10.5 20.5 3.5 13.5A5.66 5.66 0 0 1 5 6l8 8a5.66 5.66 0 0 1-2.5 6.5Z', 'M8.5 8.5 19 19'] },
  { id: 'thermometer', label: 'Termometro', keywords: ['thermometer', 'fever'], paths: ['M14 4v10.54a4 4 0 1 1-4 0V4a2 2 0 0 1 4 0Z'] },
  { id: 'umbrella', label: 'Ombrello', keywords: ['umbrella', 'rain'], paths: ['M12 22v-6', 'M12 2a7 7 0 0 0-7 7h14a7 7 0 0 0-7-7Z'] },
  { id: 'mountain', label: 'Montagna', keywords: ['mountain'], paths: ['M8 3l4 8 5-5 5 15H2L8 3Z'] },
  { id: 'tent', label: 'Tenda', keywords: ['tent', 'camp'], paths: ['M3 21 12 3l9 18', 'M7 21h10'] },
  { id: 'campfire', label: 'Fuoco campo', keywords: ['campfire'], paths: ['M8.5 14.5A2.5 2.5 0 0 0 11 12c0-1.38-.5-2-1-3-1.072-2.143-.224-4.054 2-6 .5 2.5 2 4.9 4 6.5 2 1.6 3 3.5 3 5.5a7 7 0 1 1-14 0c0-1.153.433-2.294 1-3a2.5 2.5 0 0 0 2.5 2.5Z'] },
]

const glyphById = new Map(TASK_LIST_GLYPHS.map((glyph) => [glyph.id, glyph]))

export function getTaskListGlyph(id: string): GlyphDefinition | undefined {
  return glyphById.get(id as TaskListGlyphId)
}

type GlyphIconProps = SVGProps<SVGSVGElement> & {
  glyphId: string
}

export const TaskListGlyphIcon = ({ glyphId, ...props }: GlyphIconProps) => {
  const glyph = getTaskListGlyph(glyphId)
  if (!glyph) return null

  return (
    <svg
      viewBox="0 0 24 24"
      width={20}
      height={20}
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...props}
    >
      {glyph.circles?.map((circle) => (
        <circle key={`${circle.cx}-${circle.cy}`} {...circle} />
      ))}
      {glyph.rects?.map((rect) => (
        <rect key={`${rect.x}-${rect.y}`} {...rect} />
      ))}
      {glyph.paths.map((path) => (
        <path key={path} d={path} />
      ))}
    </svg>
  )
}

export function filterTaskListGlyphs(query: string): GlyphDefinition[] {
  const normalized = query.trim().toLowerCase()
  if (!normalized) return TASK_LIST_GLYPHS
  return TASK_LIST_GLYPHS.filter((glyph) => {
    if (glyph.label.toLowerCase().includes(normalized)) return true
    if (glyph.id.includes(normalized)) return true
    return glyph.keywords.some((keyword) => keyword.includes(normalized))
  })
}
