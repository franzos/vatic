(use-modules (guix packages)
             (guix search-paths)
             (gnu packages commencement)
             (gnu packages pkg-config)
             (gnu packages freedesktop)
             (gnu packages xdisorg)
             (gnu packages vulkan)
             (gnu packages rust)
             (gnu packages linux)
             (gnu packages node)
             (gnu packages tls))

(define openssl-with-dir
  (package
    (inherit openssl)
    (native-search-paths
     (cons (search-path-specification
            (variable "OPENSSL_DIR")
            (files '("."))
            (file-type 'directory)
            (separator #f))
           (package-native-search-paths openssl)))))

(packages->manifest
 (list rust
       (list rust "cargo")
       gcc-toolchain
       pkg-config
       openssl-with-dir
       wayland
       wayland-protocols
       libxkbcommon
       vulkan-loader
       eudev
       node))
