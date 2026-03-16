import React, {createContext, useContext, useEffect, useState} from 'react';
import AsyncStorage from '@react-native-async-storage/async-storage';

interface AuthState {
  token: string | null;
  address: string | null;
  isConnected: boolean;
  connect: (token: string, address: string) => Promise<void>;
  disconnect: () => Promise<void>;
}

const AuthContext = createContext<AuthState>({
  token: null, address: null, isConnected: false,
  connect: async () => {}, disconnect: async () => {},
});

export function AuthProvider({children}: {children: React.ReactNode}) {
  const [token, setToken] = useState<string | null>(null);
  const [address, setAddress] = useState<string | null>(null);

  useEffect(() => {
    AsyncStorage.multiGet(['yeet_token', 'yeet_address']).then(pairs => {
      const t = pairs[0][1], a = pairs[1][1];
      if (t && a) { setToken(t); setAddress(a); }
    });
  }, []);

  const connect = async (t: string, a: string) => {
    await AsyncStorage.multiSet([['yeet_token', t], ['yeet_address', a]]);
    setToken(t); setAddress(a);
  };

  const disconnect = async () => {
    await AsyncStorage.multiRemove(['yeet_token', 'yeet_address']);
    setToken(null); setAddress(null);
  };

  return (
    <AuthContext.Provider value={{token, address, isConnected: !!token, connect, disconnect}}>
      {children}
    </AuthContext.Provider>
  );
}

export const useAuth = () => useContext(AuthContext);