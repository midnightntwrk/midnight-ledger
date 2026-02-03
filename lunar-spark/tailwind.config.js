/** @type {import('tailwindcss').Config} */
module.exports = {
  content: [
    './App.{js,jsx,ts,tsx}',
    './app/**/*.{js,jsx,ts,tsx}',
    './src/**/*.{js,jsx,ts,tsx}',
  ],
  presets: [require('nativewind/preset')],
  theme: {
    extend: {
      colors: {
        // Backgrounds
        background: {
          primary: '#0A0A0A',
          secondary: '#171717',
          tertiary: '#262626',
          elevated: '#1F1F1F',
        },
        // Text colors
        foreground: {
          primary: '#FFFFFF',
          secondary: '#A3A3A3',
          tertiary: '#737373',
          disabled: '#525252',
        },
        // Border colors
        border: {
          DEFAULT: '#262626',
          subtle: '#1F1F1F',
          strong: '#404040',
        },
        // Brand/Accent - Purple
        accent: {
          DEFAULT: '#A855F7',
          hover: '#9333EA',
          light: '#C084FC',
          dark: '#7E22CE',
        },
        // Status colors
        success: {
          DEFAULT: '#22C55E',
          light: '#4ADE80',
          dark: '#16A34A',
        },
        warning: {
          DEFAULT: '#F59E0B',
          light: '#FBBF24',
          dark: '#D97706',
        },
        error: {
          DEFAULT: '#EF4444',
          light: '#F87171',
          dark: '#DC2626',
        },
        info: {
          DEFAULT: '#3B82F6',
          light: '#60A5FA',
          dark: '#2563EB',
        },
        // Wallet-specific colors
        wallet: {
          shielded: '#A855F7',
          unshielded: '#3B82F6',
          dust: '#EAB308',
        },
      },
      fontFamily: {
        sans: ['System'],
        mono: ['Menlo', 'Monaco', 'Courier New', 'monospace'],
      },
      fontSize: {
        // Display
        'display-lg': ['40px', { lineHeight: '48px', fontWeight: '700' }],
        'display-md': ['32px', { lineHeight: '40px', fontWeight: '700' }],
        'display-sm': ['28px', { lineHeight: '36px', fontWeight: '700' }],
        // Headline
        'headline-lg': ['24px', { lineHeight: '32px', fontWeight: '600' }],
        'headline-md': ['20px', { lineHeight: '28px', fontWeight: '600' }],
        'headline-sm': ['18px', { lineHeight: '24px', fontWeight: '600' }],
        // Body
        'body-lg': ['16px', { lineHeight: '24px', fontWeight: '400' }],
        'body-md': ['14px', { lineHeight: '20px', fontWeight: '400' }],
        'body-sm': ['12px', { lineHeight: '16px', fontWeight: '400' }],
        // Label
        'label-lg': ['14px', { lineHeight: '20px', fontWeight: '600' }],
        'label-md': ['12px', { lineHeight: '16px', fontWeight: '600' }],
        'label-sm': ['10px', { lineHeight: '14px', fontWeight: '600' }],
      },
      spacing: {
        '4.5': '18px',
        '13': '52px',
        '15': '60px',
        '18': '72px',
      },
      borderRadius: {
        '4xl': '32px',
      },
    },
  },
  plugins: [],
};
