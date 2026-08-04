const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const reveals = document.querySelectorAll(".reveal");

if (reducedMotion || !("IntersectionObserver" in window)) {
  reveals.forEach((element) => element.classList.add("visible"));
} else {
  const observer = new IntersectionObserver(
    (entries) => {
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("visible");
          observer.unobserve(entry.target);
        }
      });
    },
    { threshold: 0.12 }
  );
  reveals.forEach((element) => observer.observe(element));
}

if (window.location.hash) {
  document.getElementById(window.location.hash.slice(1))?.scrollIntoView();
  document.fonts.ready.then(() => {
    document.getElementById(window.location.hash.slice(1))?.scrollIntoView();
  });
}
