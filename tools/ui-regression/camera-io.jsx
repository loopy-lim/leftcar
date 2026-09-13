import React,{useState,useCallback} from 'react';
export * from './viewer-io.js';
const io={permission:null,requests:[],settings:[],listeners:new Set()};window.cameraIo=io;
export function useCameraPermissions(){const[p,set]=useState(io.permission);io.setPermission=set;const read=useCallback(()=>new Promise((resolve,reject)=>io.requests.push({resolve(value){set(value);resolve(value);},reject})),[]);return [p,read,read];}
export const AppState={addEventListener(_event,listener){io.listeners.add(listener);return {remove(){io.listeners.delete(listener);}};}};
export const openSettings=()=>new Promise((resolve,reject)=>io.settings.push({resolve,reject}));
export const useLocalSearchParams=()=>({});
export const useWindowDimensions=()=>({width:800,height:1000});
export const useColorScheme=()=> 'light';
export const StyleSheet={create:x=>x};
const box=tag=>function Box({children,onPress,accessibilityLabel,accessibilityRole,disabled,value,onChangeText,..._rest}){return React.createElement(tag,{onClick:onPress,'aria-label':accessibilityLabel,role:accessibilityRole,disabled,value,onChange:onChangeText?e=>onChangeText(e.target.value):undefined},children);};
export const View=box('div'),Text=box('span'),Pressable=box('button'),ScrollView=box('div'),SafeAreaView=box('div'),TextInput=box('input');
export const ActivityIndicator=()=>React.createElement('span',null,'Loading');
export const Ionicons=()=>null;
export const CameraView=()=>React.createElement('div',{'data-testid':'live-camera'},'Controlled camera Surface');
