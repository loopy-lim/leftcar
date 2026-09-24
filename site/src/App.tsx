import { Hero } from './components/Hero';
import { FeatureGrid } from './components/FeatureGrid';
import { Security } from './components/Security';
import { StackBadges } from './components/StackBadges';
import { Status } from './components/Status';
import { Footer } from './components/Footer';

function App() {
  return (
    <>
      <Hero />
      <main>
        <FeatureGrid />
        <Security />
        <StackBadges />
        <Status />
      </main>
      <Footer />
    </>
  );
}

export default App;
