import React from 'react';
import {NavigationContainer} from '@react-navigation/native';
import {createBottomTabNavigator} from '@react-navigation/bottom-tabs';
import {createNativeStackNavigator} from '@react-navigation/native-stack';
import {SafeAreaProvider} from 'react-native-safe-area-context';
import {StatusBar} from 'react-native';

import {FeedScreen} from './src/screens/FeedScreen';
import {ComposeScreen} from './src/screens/ComposeScreen';
import {ProfileScreen} from './src/screens/ProfileScreen';
import {WalletScreen} from './src/screens/WalletScreen';
import {PostDetailScreen} from './src/screens/PostDetailScreen';
import {AuthProvider} from './src/context/AuthContext';

const Tab = createBottomTabNavigator();
const Stack = createNativeStackNavigator();

function HomeTabs() {
  return (
    <Tab.Navigator
      screenOptions={{
        headerShown: false,
        tabBarStyle: {
          backgroundColor: '#0c0e14',
          borderTopColor: 'rgba(255,255,255,0.07)',
          height: 60,
        },
        tabBarActiveTintColor: '#c6f135',
        tabBarInactiveTintColor: '#4a5068',
        tabBarLabelStyle: {
          fontFamily: 'DMMonoRegular',
          fontSize: 10,
          textTransform: 'uppercase',
          letterSpacing: 0.8,
        },
      }}>
      <Tab.Screen
        name="Feed"
        component={FeedScreen}
        options={{tabBarIcon: ({color}) => <TabIcon name="⚡" color={color} />}}
      />
      <Tab.Screen
        name="Compose"
        component={ComposeScreen}
        options={{tabBarIcon: ({color}) => <TabIcon name="+" color={color} />}}
      />
      <Tab.Screen
        name="Wallet"
        component={WalletScreen}
        options={{tabBarIcon: ({color}) => <TabIcon name="◎" color={color} />}}
      />
      <Tab.Screen
        name="Profile"
        component={ProfileScreen}
        options={{tabBarIcon: ({color}) => <TabIcon name="○" color={color} />}}
      />
    </Tab.Navigator>
  );
}

import {Text} from 'react-native';
function TabIcon({name, color}: {name: string; color: string}) {
  return <Text style={{color, fontSize: 18}}>{name}</Text>;
}

export default function App() {
  return (
    <SafeAreaProvider>
      <AuthProvider>
        <NavigationContainer>
          <StatusBar barStyle="light-content" backgroundColor="#05060a" />
          <Stack.Navigator screenOptions={{headerShown: false}}>
            <Stack.Screen name="Home" component={HomeTabs} />
            <Stack.Screen name="PostDetail" component={PostDetailScreen} />
          </Stack.Navigator>
        </NavigationContainer>
      </AuthProvider>
    </SafeAreaProvider>
  );
}