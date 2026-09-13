import React,{useState} from 'react';import {createRoot} from 'react-dom/client';
import Pairing from '../../apps/viewer-expo/app/pairing';
import {LanguageProvider} from '../../apps/viewer-expo/src/i18n';
function Fixture(){const[mounted,setMounted]=useState(true);window.mountCamera=setMounted;return <LanguageProvider>{mounted?<Pairing/>:null}</LanguageProvider>;}
createRoot(document.getElementById('root')).render(<Fixture/>);
