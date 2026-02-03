# Lunar Spark UI Revamp - Modern Fintech Redesign

## Overview
Complete redesign of the React Native wallet app with modern fintech aesthetics (Revolut/Coinbase style).

## Tech Stack

| Category | Choice | Rationale |
|----------|--------|-----------|
| Navigation | **Expo Router v4** | File-based routing, built-in deep linking, works great with Expo 54 |
| Styling | **NativeWind v4** | Tailwind utility classes, excellent dark/light mode, fast development |
| Animations | **Reanimated 4 + Moti** | 60fps UI-thread animations, declarative API |
| Icons | **Lucide React Native** | Clean, consistent icon set |

## Design System

### Color Palette (Dark Theme - Default)
```
Backgrounds: #0A0A0A (primary), #171717 (cards), #262626 (inputs)
Text: #FFFFFF (primary), #A3A3A3 (secondary), #737373 (tertiary)
Accent: #A855F7 (purple - CTAs and brand)
Status: #22C55E (success), #F59E0B (warning), #EF4444 (error)
Wallets: #A855F7 (shielded), #3B82F6 (unshielded), #EAB308 (dust)
```

### Typography
- Display: 28-40px, bold - hero numbers
- Headline: 18-24px, semibold - section headers
- Body: 12-16px, regular - content
- Mono: 12-16px, monospace - addresses/numbers

### Token Naming
- **Shielded**: No main token (supports multiple tokens)
- **Unshielded**: tNIGHT
- **Dust**: tDUST (network fees only, not transferrable)

## Navigation Structure

```
/app
├── (auth)/
│   ├── welcome.tsx          # Onboarding
│   └── setup.tsx            # Network + seed input
├── (tabs)/
│   ├── index.tsx            # Home - portfolio overview
│   ├── wallets.tsx          # Wallet details (3 cards)
│   ├── activity.tsx         # Transaction history (stub)
│   └── settings.tsx         # Network, theme, disconnect
└── (modals)/
    ├── send/
    │   ├── index.tsx        # Send flow - select wallet & amount
    │   ├── recipient.tsx    # Enter/scan recipient address
    │   ├── confirm.tsx      # Review & confirm transaction
    │   └── success.tsx      # Transaction submitted confirmation
    └── receive/
        └── index.tsx        # Receive - QR code + address display
```

## Component Architecture

### Design System (`/src/design-system/`)
```
tokens/         → colors, typography, spacing, shadows
components/
  ├── Skeleton, SkeletonText, SkeletonCard
  └── (more to come)
theme/
  └── ThemeProvider.tsx
```

### Providers (`/src/providers/`)
```
WalletProvider  → Wallet state management with simplified WalletData interface
```

## Implementation Phases

### Phase 1: Foundation ✅
- [x] Install dependencies (expo-router, nativewind, reanimated, moti, lucide)
- [x] Configure NativeWind + Tailwind
- [x] Create design tokens (`/src/design-system/tokens/`)
- [x] Set up ThemeProvider with dark/light support
- [x] Initialize Expo Router file structure
- [x] Build primitive components (Skeleton)

### Phase 2: Component Library (Partial)
- [x] Build wallet-specific components (WalletCard in wallets.tsx)
- [x] Add animations with Moti (skeleton loaders, transitions)
- [ ] Build core UI components (Card, TextInput, Screen, Header)
- [ ] Build feedback components (Spinner, ProgressBar, Toast)
- [ ] Build custom TabBar component

### Phase 3: Screen Migration ✅
- [x] Create Welcome screen (onboarding)
- [x] Migrate Setup screen to new design
- [x] Build Dashboard (Home tab) - portfolio overview
- [x] Migrate WalletDashboard to Wallets tab
- [x] Build Settings tab (network selector, theme toggle, disconnect)
- [x] Build Send flow (wallet select → amount → recipient → confirm → success)
- [x] Build Receive screen (QR code display + address copy)
- [ ] Add QR scanner for recipient address input

### Phase 4: Polish
- [x] Add screen transitions (fade animations)
- [x] Implement haptic feedback on buttons
- [ ] Add pull-to-refresh on wallets
- [ ] Implement toast notifications
- [ ] Add sync progress animations
- [ ] Test on multiple device sizes

## Key Files Modified

| File | Changes |
|------|---------|
| `package.json` | Added expo-router, nativewind, moti, lucide, reanimated, worklets |
| `babel.config.js` | Added NativeWind preset |
| `metro.config.js` | Added NativeWind wrapper |
| `tailwind.config.js` | New - Tailwind configuration with design tokens |
| `global.css` | New - Tailwind imports |
| `src/providers/WalletProvider.tsx` | Added WalletData/WalletSummary interfaces |

## Files Created

```
lunar-spark/
├── app/
│   ├── _layout.tsx
│   ├── index.tsx
│   ├── (auth)/_layout.tsx
│   ├── (auth)/welcome.tsx
│   ├── (auth)/setup.tsx
│   ├── (tabs)/_layout.tsx
│   ├── (tabs)/index.tsx
│   ├── (tabs)/wallets.tsx
│   ├── (tabs)/activity.tsx
│   ├── (tabs)/settings.tsx
│   ├── (modals)/send/index.tsx
│   ├── (modals)/send/recipient.tsx
│   ├── (modals)/send/confirm.tsx
│   ├── (modals)/send/success.tsx
│   └── (modals)/receive/index.tsx
├── src/design-system/
│   ├── index.ts
│   ├── tokens/colors.ts
│   ├── theme/ThemeProvider.tsx
│   └── components/Skeleton.tsx
├── tailwind.config.js
└── global.css
```

## Remaining Work

1. **QR Scanner**: Implement camera-based QR code scanning for recipient addresses
2. **Toast Notifications**: Add feedback for copy actions, errors, etc.
3. **Pull to Refresh**: Add refresh gesture on wallets/home screens
4. **Activity Tab**: Implement transaction history display
5. **Actual Transaction Logic**: Connect send flow to wallet facade
6. **Light Theme**: Implement and test light mode colors
7. **Error Handling**: Improve error states and recovery flows
