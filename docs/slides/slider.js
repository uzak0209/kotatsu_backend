// Simple slide navigation for screen viewing
// PDF export: Just use browser print (Ctrl+P / Cmd+P)

(function () {
  'use strict';

  // Get all slides
  const slides = document.querySelectorAll('.slide');
  if (!slides.length) return;

  let currentSlide = 0;

  // Scroll to a specific slide
  function scrollToSlide(index) {
    if (index < 0 || index >= slides.length) return;
    currentSlide = index;
    slides[index].scrollIntoView({ behavior: 'smooth', block: 'start' });
  }

  // Navigate to next slide
  function nextSlide() {
    if (currentSlide < slides.length - 1) {
      scrollToSlide(currentSlide + 1);
    }
  }

  // Navigate to previous slide
  function prevSlide() {
    if (currentSlide > 0) {
      scrollToSlide(currentSlide - 1);
    }
  }

  // Update current slide based on scroll position
  function updateCurrentSlide() {
    const scrollPosition = window.scrollY + window.innerHeight / 2;

    slides.forEach((slide, index) => {
      const rect = slide.getBoundingClientRect();
      const slideTop = rect.top + window.scrollY;
      const slideBottom = slideTop + rect.height;

      if (scrollPosition >= slideTop && scrollPosition < slideBottom) {
        currentSlide = index;
      }
    });
  }

  // Keyboard navigation
  document.addEventListener('keydown', (e) => {
    // Don't interfere if user is typing
    if (e.target.matches('input, textarea, select')) return;

    switch (e.key) {
      case 'ArrowDown':
      case 'ArrowRight':
      case ' ':
      case 'j':
      case 'l':
      case 'PageDown':
        e.preventDefault();
        nextSlide();
        break;

      case 'ArrowUp':
      case 'ArrowLeft':
      case 'k':
      case 'h':
      case 'PageUp':
        e.preventDefault();
        prevSlide();
        break;

      case 'Home':
        e.preventDefault();
        scrollToSlide(0);
        break;

      case 'End':
        e.preventDefault();
        scrollToSlide(slides.length - 1);
        break;
    }
  });

  // Mouse wheel navigation (with debounce)
  let wheelTimeout;
  let isWheeling = false;

  document.addEventListener('wheel', (e) => {
    if (isWheeling) return;

    clearTimeout(wheelTimeout);
    wheelTimeout = setTimeout(() => {
      isWheeling = false;
    }, 100);

    if (Math.abs(e.deltaY) > 50) {
      isWheeling = true;

      if (e.deltaY > 0) {
        nextSlide();
      } else {
        prevSlide();
      }
    }
  }, { passive: true });

  // Update current slide on scroll
  let scrollTimeout;
  window.addEventListener('scroll', () => {
    clearTimeout(scrollTimeout);
    scrollTimeout = setTimeout(updateCurrentSlide, 100);
  }, { passive: true });

  // Touch/swipe support for mobile
  let touchStartY = 0;
  let touchStartX = 0;

  document.addEventListener('touchstart', (e) => {
    touchStartY = e.touches[0].clientY;
    touchStartX = e.touches[0].clientX;
  }, { passive: true });

  document.addEventListener('touchend', (e) => {
    const touchEndY = e.changedTouches[0].clientY;
    const touchEndX = e.changedTouches[0].clientX;

    const deltaY = touchStartY - touchEndY;
    const deltaX = touchStartX - touchEndX;

    // Only handle vertical swipes
    if (Math.abs(deltaY) > Math.abs(deltaX) && Math.abs(deltaY) > 50) {
      if (deltaY > 0) {
        nextSlide();
      } else {
        prevSlide();
      }
    }
  }, { passive: true });

  // Initialize
  updateCurrentSlide();

  // Log for debugging
  console.log(`Loaded ${slides.length} slides`);
  console.log('Navigation: Arrow keys, Space, j/k, Mouse wheel, Touch swipe');
  console.log('PDF Export: Ctrl+P (Cmd+P) → Landscape, No margins, Background graphics ON');
})();
