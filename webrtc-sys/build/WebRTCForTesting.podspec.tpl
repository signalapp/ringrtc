Pod::Spec.new do |s|
  s.name     = "WebRTCForTesting"
  s.version  = "0.0.1"
  s.summary  = "Intended only for testing SignalRingRTC within this repository"
  s.license  = "BSD"
  s.homepage = "https://github.com/signalapp/webrtc"
  s.source   = {{ git: "https://github.com/signalapp/webrtc.git" }}
  s.author   = {{ "iOS Team": "ios@signal.org" }}

  s.platform = :ios, "'{}'"
  s.vendored_frameworks = "'{}'/WebRTC.xcframework"
end
