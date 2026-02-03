import { Stack } from 'expo-router';

export default function ModalsLayout() {
  return (
    <Stack
      screenOptions={{
        headerShown: false,
        presentation: 'modal',
        contentStyle: { backgroundColor: '#0A0A0A' },
        animation: 'slide_from_bottom',
      }}
    />
  );
}
