import { View, ViewProps, DimensionValue } from 'react-native';
import { MotiView } from 'moti';

interface SkeletonProps extends ViewProps {
  width?: DimensionValue;
  height?: DimensionValue;
  rounded?: 'sm' | 'md' | 'lg' | 'full';
}

const roundedStyles = {
  sm: 'rounded',
  md: 'rounded-lg',
  lg: 'rounded-xl',
  full: 'rounded-full',
};

export function Skeleton({
  width = '100%',
  height = 20,
  rounded = 'md',
  className = '',
  ...props
}: SkeletonProps) {
  return (
    <MotiView
      from={{ opacity: 0.5 }}
      animate={{ opacity: 1 }}
      transition={{
        type: 'timing',
        duration: 1000,
        loop: true,
      }}
      className={`bg-background-tertiary ${roundedStyles[rounded]} ${className}`}
      style={{ width, height }}
      {...props}
    />
  );
}

export function SkeletonText({ lines = 3 }: { lines?: number }) {
  return (
    <View className="gap-2">
      {Array.from({ length: lines }).map((_, i) => (
        <Skeleton
          key={i}
          height={16}
          width={i === lines - 1 ? '60%' : '100%'}
          rounded="sm"
        />
      ))}
    </View>
  );
}

export function SkeletonCard() {
  return (
    <View className="bg-background-secondary rounded-2xl p-4 border border-border">
      <View className="flex-row items-center mb-4">
        <Skeleton width={48} height={48} rounded="full" />
        <View className="flex-1 ml-3">
          <Skeleton width="60%" height={18} className="mb-2" />
          <Skeleton width="40%" height={14} />
        </View>
      </View>
      <SkeletonText lines={2} />
    </View>
  );
}
